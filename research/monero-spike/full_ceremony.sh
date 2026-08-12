#!/usr/bin/env bash
# DUCAT Monero spike — the complete 2-of-3 multisig ceremony, including the
# out-of-band steps that turn out to be mandatory.
#
# Tests O1 against §17.2's FLOAT { user, market_arbiter_set, recovery }.
#
# The shape of this script IS the finding. A ceremony that §17.1 describes as
# "calm, retryable onboarding" requires, per participant:
#
#   1. create the wallet with monero-wallet-cli (not the RPC)
#   2. set enable-multisig-experimental 1 via CLI  — no RPC method exists
#   3. start wallet-rpc
#   4. prepare_multisig, make_multisig
#   5. STOP wallet-rpc, because make_multisig silently reset the flag to 0 and
#      the file cannot be edited while the service holds it
#   6. re-set the flag via CLI, again
#   7. restart wallet-rpc
#   8. exchange_multisig_keys until ready
set -uo pipefail

BIN="$PWD/monero-x86_64-linux-gnu-v0.18.5.1"
NODE=node.monerodevs.org:38089
PORTS=(28088 28089 28090)
NAMES=(user arbiter recovery)
PW=""
t0=$(date +%s)
CLI_STEPS=0
RESTARTS=0

cli() { # wallet, args...
  CLI_STEPS=$((CLI_STEPS+1))
  local w=$1; shift
  timeout 90 "$BIN/monero-wallet-cli" --stagenet --offline \
    --wallet-file "$w" --password "$PW" --command "$@" 2>&1
}

flag_of() { cli "$1" set 2>/dev/null | grep -oE 'enable-multisig-experimental = [01]' | grep -oE '[01]$'; }

start_rpc() {
  local i=0
  for name in "${NAMES[@]}"; do
    setsid nohup "$BIN/monero-wallet-rpc" --stagenet \
      --daemon-address "$NODE" --untrusted-daemon \
      --rpc-bind-port "${PORTS[$i]}" --disable-rpc-login \
      --wallet-dir "$PWD/w_$name" --log-file "$PWD/w_$name/rpc.log" \
      < /dev/null > "$PWD/w_$name/rpc.out" 2>&1 &
    disown
    i=$((i+1))
  done
  RESTARTS=$((RESTARTS+1))
  sleep 15
}

stop_rpc() {
  for p in $(pgrep -f "gnu-v0.18.5.1/monero-wallet-rpc" 2>/dev/null); do kill -9 "$p" 2>/dev/null; done
  sleep 5
}

call() {
  local out err
  out=$(curl -s -m 60 -X POST "http://127.0.0.1:$1/json_rpc" \
        -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$2\",\"params\":$3}")
  [ -n "$out" ] || { echo "FAIL $4: empty response" >&2; return 1; }
  err=$(echo "$out" | jq -r '.error.message // empty')
  [ -z "$err" ] || { echo "FAIL $4: $err" >&2; return 1; }
  echo "$out"
}

open_all() {
  for i in 0 1 2; do
    call "${PORTS[$i]}" open_wallet \
      "{\"filename\":\"ms_${NAMES[$i]}\",\"password\":\"$PW\"}" "open ${NAMES[$i]}" >/dev/null || return 1
  done
}

stop_rpc

echo "=== 1. create wallets via CLI and enable the experimental flag ==="
for n in "${NAMES[@]}"; do
  rm -rf "w_$n"; mkdir -p "w_$n"
  timeout 90 "$BIN/monero-wallet-cli" --stagenet --offline \
    --generate-new-wallet "w_$n/ms_$n" --password "$PW" --mnemonic-language English \
    --command set enable-multisig-experimental 1 >/dev/null 2>&1
  CLI_STEPS=$((CLI_STEPS+1))
  echo "  $n: flag=$(flag_of "w_$n/ms_$n")"
done

echo
echo "=== 2. start wallet-rpc (restart #$((RESTARTS+1))) ==="
start_rpc
open_all || exit 1
echo "  three instances up, wallets open"

echo
echo "=== 3. prepare_multisig ==="
declare -A INFO
for i in 0 1 2; do
  r=$(call "${PORTS[$i]}" prepare_multisig '{}' "prepare ${NAMES[$i]}") || exit 1
  INFO[$i]=$(echo "$r" | jq -r '.result.multisig_info')
  echo "  ${NAMES[$i]}: ${#INFO[$i]} chars"
done

peers_of() {
  local me=$1 out=()
  for j in 0 1 2; do [ "$j" = "$me" ] || out+=("\"${INFO[$j]}\""); done
  local IFS=,; echo "[${out[*]}]"
}

echo
echo "=== 4. make_multisig (2 of 3) ==="
declare -A NEXT
for i in 0 1 2; do
  r=$(call "${PORTS[$i]}" make_multisig \
      "{\"multisig_info\":$(peers_of $i),\"threshold\":2,\"password\":\"$PW\"}" \
      "make ${NAMES[$i]}") || exit 1
  NEXT[$i]=$(echo "$r" | jq -r '.result.multisig_info // empty')
  echo "  ${NAMES[$i]}: next=${#NEXT[$i]} chars"
done
for i in 0 1 2; do INFO[$i]="${NEXT[$i]}"; done
printf '%s\n' "${INFO[0]}" "${INFO[1]}" "${INFO[2]}" > make_info.txt

echo
echo "=== 5. stop wallet-rpc so the wallet files can be edited ==="
stop_rpc
for i in 0 1 2; do
  echo "  ${NAMES[$i]}: flag after make_multisig = $(flag_of "w_${NAMES[$i]}/ms_${NAMES[$i]}")"
done

echo
echo "=== 6. re-enable the flag that make_multisig cleared ==="
for i in 0 1 2; do
  cli "w_${NAMES[$i]}/ms_${NAMES[$i]}" set enable-multisig-experimental 1 >/dev/null
  echo "  ${NAMES[$i]}: flag=$(flag_of "w_${NAMES[$i]}/ms_${NAMES[$i]}")"
done

echo
echo "=== 7. restart wallet-rpc (restart #$((RESTARTS+1))) ==="
start_rpc
open_all || exit 1

echo
echo "=== 8. exchange_multisig_keys ==="
ROUND=1
while :; do
  DONE=1
  for i in 0 1 2; do
    st=$(call "${PORTS[$i]}" is_multisig '{}' "state ${NAMES[$i]}") || exit 1
    [ "$(echo "$st" | jq -r '.result.ready')" = "true" ] || DONE=0
  done
  [ "$DONE" = "1" ] && break

  for i in 0 1 2; do
    r=$(call "${PORTS[$i]}" exchange_multisig_keys \
        "{\"multisig_info\":$(peers_of $i),\"password\":\"$PW\"}" \
        "exchange r$ROUND ${NAMES[$i]}") || exit 1
    NEXT[$i]=$(echo "$r" | jq -r '.result.multisig_info // empty')
    echo "  r$ROUND ${NAMES[$i]}: addr=$(echo "$r" | jq -r '.result.address // "-"' | cut -c1-22)... next=${#NEXT[$i]}"
  done
  for i in 0 1 2; do INFO[$i]="${NEXT[$i]}"; done
  ROUND=$((ROUND+1))
  [ "$ROUND" -gt 6 ] && { echo "FAIL: no convergence in 6 rounds" >&2; exit 1; }
done

echo
echo "=== 9. result ==="
declare -A ADDR
for i in 0 1 2; do
  st=$(call "${PORTS[$i]}" is_multisig '{}' "final ${NAMES[$i]}") || exit 1
  a=$(call "${PORTS[$i]}" get_address '{"account_index":0}' "addr ${NAMES[$i]}") || exit 1
  ADDR[$i]=$(echo "$a" | jq -r '.result.address')
  echo "  ${NAMES[$i]}: $(echo "$st" | jq -c '.result')"
done

t1=$(date +%s)
echo
if [ "${ADDR[0]}" = "${ADDR[1]}" ] && [ "${ADDR[1]}" = "${ADDR[2]}" ]; then
  echo "CONVERGED — all three parties agree:"
  echo "  ${ADDR[0]}"
else
  echo "MISMATCH:"; for i in 0 1 2; do echo "  ${NAMES[$i]} ${ADDR[$i]}"; done
fi
echo "  exchange rounds:        $((ROUND-1))"
echo "  out-of-band CLI steps:  $CLI_STEPS"
echo "  wallet-rpc restarts:    $RESTARTS"
echo "  wall clock:             $((t1-t0))s"
