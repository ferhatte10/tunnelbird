#!/bin/sh
# Creates, shows or deletes the throwaway OpenVPN test server on AWS.
#
#   build-aux/test-server/aws.sh up       create it (or reuse it), install
#                                         OpenVPN, fetch the client profiles
#   build-aux/test-server/aws.sh status   its address and state
#   build-aux/test-server/aws.sh down     delete everything it created
#
# Needs AWS credentials (`aws login`). Creates, all named
# “tunnelbird-openvpn-test”: an EC2 key pair from your *public* SSH key, a
# security group open to your current public address only (SSH and UDP 1194),
# and one t4g.micro Ubuntu 24.04 instance (Free Tier eligible: the free plan
# refuses other types). Run `down` when the tests are over.
#
# Settings (environment): AWS_REGION (default eu-north-1, Stockholm, so the
# app shows another country; some AWS organisations only allow one region),
# SSH_KEY (private key path, default ~/.ssh/id_ed25519; only its .pub is sent), OUT (where profiles go,
# default ~/tunnelbird-openvpn-test, outside the repository).
set -eu

REGION=${AWS_REGION:-eu-north-1}
NAME=tunnelbird-openvpn-test
KEY=${SSH_KEY:-$HOME/.ssh/id_ed25519}
OUT=${OUT:-$HOME/tunnelbird-openvpn-test}
TYPE=t4g.micro
HERE=$(cd "$(dirname "$0")" && pwd)

# No pager: the AWS CLI would otherwise stop on its JSON answers (`down`).
export AWS_PAGER=""
ec2() { aws --region "$REGION" ec2 "$@"; }
say() { printf '==> %s\n' "$*"; }

instance_id() {
  ec2 describe-instances \
    --filters "Name=tag:Name,Values=$NAME" "Name=instance-state-name,Values=pending,running,stopping,stopped" \
    --query 'Reservations[].Instances[0].InstanceId' --output text
}

public_ip() {
  ec2 describe-instances --instance-ids "$1" \
    --query 'Reservations[0].Instances[0].PublicIpAddress' --output text
}

ssh_to() {
  host=$1
  shift
  ssh -i "$KEY" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5 "ubuntu@$host" "$@"
}

up() {
  aws sts get-caller-identity >/dev/null || { echo "Not signed in to AWS: run aws login" >&2; exit 1; }
  [ -f "$KEY.pub" ] || { echo "No public key $KEY.pub (set SSH_KEY)" >&2; exit 1; }

  say "Key pair (public key only)"
  ec2 describe-key-pairs --key-names "$NAME" >/dev/null 2>&1 ||
    ec2 import-key-pair --key-name "$NAME" --public-key-material "fileb://$KEY.pub" >/dev/null

  say "Security group, open to this computer's address only"
  my_ip=$(curl -fsS https://checkip.amazonaws.com)
  vpc=$(ec2 describe-vpcs --filters Name=is-default,Values=true --query 'Vpcs[0].VpcId' --output text)
  [ "$vpc" != None ] || { echo "No default VPC in $REGION" >&2; exit 1; }
  sg=$(ec2 describe-security-groups --filters "Name=group-name,Values=$NAME" "Name=vpc-id,Values=$vpc" \
    --query 'SecurityGroups[0].GroupId' --output text)
  if [ "$sg" = None ]; then
    sg=$(ec2 create-security-group --group-name "$NAME" --vpc-id "$vpc" \
      --description "Tunnelbird OpenVPN test server" --query GroupId --output text)
  fi
  for rule in tcp,22 udp,1194; do
    ec2 authorize-security-group-ingress --group-id "$sg" \
      --protocol "${rule%,*}" --port "${rule#*,}" --cidr "$my_ip/32" >/dev/null 2>&1 || true
  done

  id=$(instance_id)
  if [ -z "$id" ] || [ "$id" = None ]; then
    say "Instance $TYPE, Ubuntu 24.04 (arm64), in $REGION"
    ami=$(aws --region "$REGION" ssm get-parameter \
      --name /aws/service/canonical/ubuntu/server/24.04/stable/current/arm64/hvm/ebs-gp3/ami-id \
      --query Parameter.Value --output text)
    id=$(ec2 run-instances --image-id "$ami" --instance-type "$TYPE" --key-name "$NAME" \
      --security-group-ids "$sg" --metadata-options HttpTokens=required \
      --block-device-mappings '[{"DeviceName":"/dev/sda1","Ebs":{"VolumeSize":8,"VolumeType":"gp3","DeleteOnTermination":true}}]' \
      --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$NAME},{Key=Project,Value=tunnelbird}]" \
      --query 'Instances[0].InstanceId' --output text)
  fi
  ec2 start-instances --instance-ids "$id" >/dev/null 2>&1 || true
  ec2 wait instance-running --instance-ids "$id"
  ip=$(public_ip "$id")

  say "Waiting for SSH on $ip"
  tries=0
  until ssh_to "$ip" true 2>/dev/null; do
    tries=$((tries + 1))
    [ "$tries" -lt 40 ] || { echo "SSH does not answer on $ip" >&2; exit 1; }
    sleep 5
  done

  say "Installing OpenVPN"
  scp -i "$KEY" -o StrictHostKeyChecking=accept-new "$HERE/openvpn.sh" "ubuntu@$ip:/tmp/openvpn.sh" >/dev/null
  ssh_to "$ip" sudo sh /tmp/openvpn.sh

  say "Fetching the client profiles"
  mkdir -p "$OUT"
  chmod 700 "$OUT"
  ssh_to "$ip" sudo tar -C /root/tunnelbird-test -cf - . | tar -C "$OUT" -xf -
  echo
  echo "Server: $ip (UDP 1194). Profiles and test credentials: $OUT"
  echo "Delete it afterwards: $0 down"
}

status() {
  id=$(instance_id)
  if [ -z "$id" ] || [ "$id" = None ]; then
    echo "No test server in $REGION."
  else
    state=$(ec2 describe-instances --instance-ids "$id" \
      --query 'Reservations[0].Instances[0].State.Name' --output text)
    echo "$id: $state, $(public_ip "$id")"
  fi
}

down() {
  id=$(instance_id)
  if [ -n "$id" ] && [ "$id" != None ]; then
    say "Deleting instance $id"
    ec2 terminate-instances --instance-ids "$id" >/dev/null
    ec2 wait instance-terminated --instance-ids "$id"
  fi
  sg=$(ec2 describe-security-groups --filters "Name=group-name,Values=$NAME" \
    --query 'SecurityGroups[0].GroupId' --output text)
  if [ "$sg" != None ]; then
    say "Deleting security group $sg"
    ec2 delete-security-group --group-id "$sg" >/dev/null
  fi
  if ec2 describe-key-pairs --key-names "$NAME" >/dev/null 2>&1; then
    say "Deleting key pair $NAME (AWS copy of the public key only)"
    ec2 delete-key-pair --key-name "$NAME" >/dev/null
  fi
  echo "Nothing left on AWS. Local profiles stay in $OUT (delete them when done)."
}

case ${1:-} in
  up) up ;;
  status) status ;;
  down) down ;;
  *) echo "Usage: $0 up|status|down" >&2; exit 2 ;;
esac
