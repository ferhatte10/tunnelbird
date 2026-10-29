// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Translation of user-visible messages. With the `gettext` feature (the Linux
//! app), messages go through the system's gettext in the app's text domain.
//! Without it (the Windows and macOS tunnel services, whose interfaces
//! translate by error code), messages stay in English.

#[cfg(feature = "gettext")]
pub use gettextrs::gettext;

/// English text as is, when gettext is not built in.
#[cfg(not(feature = "gettext"))]
pub fn gettext<T: Into<String>>(msgid: T) -> String {
    msgid.into()
}
