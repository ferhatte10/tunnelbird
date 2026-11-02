// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

// One connection to the tunnel service: one JSON object per line on a Unix
// socket. A thread reads; answers resume their request, events go to `onEvent`.

import Foundation

final class ServiceConnection: @unchecked Sendable {
    private let fd: Int32
    private let lock = NSLock()
    private var nextID: UInt64 = 0
    private var waiting: [UInt64: CheckedContinuation<ServiceMessage, Error>] = [:]
    private var closed = false

    /// Connects, or throws if the service is not there.
    init(path: String) throws {
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw ServiceError(code: "internal", details: "socket") }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard path.utf8.count < capacity else {
            close(fd)
            throw ServiceError(code: "internal", details: "Socket path too long")
        }
        withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            buffer.copyBytes(from: path.utf8)
        }
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard result == 0 else {
            let reason = String(cString: strerror(errno))
            close(fd)
            throw ServiceError(code: "unavailable", details: reason)
        }
    }

    /// Reads until the service goes away. `onClose` is then called once.
    func start(onEvent: @escaping @Sendable (ServiceMessage) -> Void,
               onClose: @escaping @Sendable () -> Void) {
        Thread.detachNewThread { [self] in
            var pending = Data()
            var chunk = [UInt8](repeating: 0, count: 65536)
            while true {
                let count = read(fd, &chunk, chunk.count)
                if count <= 0 { break }
                pending.append(contentsOf: chunk[0..<count])
                while let end = pending.firstIndex(of: 10) {
                    let line = pending[pending.startIndex..<end]
                    pending.removeSubrange(pending.startIndex...end)
                    guard let message = try? JSONDecoder().decode(ServiceMessage.self, from: line) else {
                        continue
                    }
                    if let id = message.id, message.response != nil {
                        resume(id, with: message)
                    } else if message.event != nil {
                        onEvent(message)
                    }
                }
            }
            finish()
            onClose()
        }
    }

    private func resume(_ id: UInt64, with message: ServiceMessage) {
        lock.lock()
        let continuation = waiting.removeValue(forKey: id)
        lock.unlock()
        if message.response == "error" {
            continuation?.resume(throwing: ServiceError(
                code: message.code ?? "internal", details: message.message ?? ""))
        } else {
            continuation?.resume(returning: message)
        }
    }

    private func finish() {
        lock.lock()
        let pending = waiting
        waiting = [:]
        closed = true
        lock.unlock()
        close(fd)
        for continuation in pending.values {
            continuation.resume(throwing: ServiceError(code: "unavailable", details: "Connection lost"))
        }
    }

    /// Sends a request and waits for its answer.
    func send(_ body: [String: Any]) async throws -> ServiceMessage {
        try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            guard !closed else {
                lock.unlock()
                continuation.resume(throwing: ServiceError(code: "unavailable", details: "Connection lost"))
                return
            }
            nextID += 1
            let id = nextID
            waiting[id] = continuation
            lock.unlock()
            var message = body
            message["id"] = id
            guard var line = try? JSONSerialization.data(withJSONObject: message) else {
                resume(id, with: ServiceMessageFactory.internalError(id))
                return
            }
            line.append(10)
            let written = line.withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
            if written != line.count {
                resume(id, with: ServiceMessageFactory.internalError(id))
            }
        }
    }

    func disconnect() {
        shutdown(fd, SHUT_RDWR)
    }
}

private enum ServiceMessageFactory {
    static func internalError(_ id: UInt64) -> ServiceMessage {
        let text = #"{"id":\#(id),"response":"error","code":"internal","message":"Cannot write to the service"}"#
        // A fixed, valid text: decoding cannot fail.
        return try! JSONDecoder().decode(ServiceMessage.self, from: Data(text.utf8))
    }
}
