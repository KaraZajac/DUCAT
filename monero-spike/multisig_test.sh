#!/usr/bin/env bash
# DUCAT Monero spike — 2-of-3 multisig setup, three independent parties.
#
# Tests open problem O1: "Monero multisig fragility makes §8.2 the riskiest
# engineering in the protocol." §17.2's FLOAT holds the bond in a 2-of-3
# multisig { user, market_arbiter_set, recovery }, and §17.1 argues the
# fragility is tolerable because setup happens once, off the critical path,
# with unlimited retries.
#
# Each party gets its own wallet-rpc instance, because that is what three
# parties on three devices actually looks like. An earlier version of this
# script drove all three from one instance with close/open churn; that is not
# the deployment, and it manufactured failures of its own.
set -uo pipefail

PORTS=(28088 28089 28090)
NAMES=(user arbiter recovery)
PW=""
CALLS=0

rpc() { # port, method, params
  CALLS=$((CALLS+1))
  curl -s -m 60 -X POST "http://127.0.0.1:$1/json_rpc" \
    -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$2\",\"params\":$3}"
}

# Strict: a response must be well-formed JSON carrying a result. Returns
# non-zero rather than calling exit, because `r=$(call ...)` runs in a subshell
# and an exit there would abort only the subshell -- letting the script sail on
# with empty key material, which is exactly the failure this function exists to
# prevent. Call sites must use `|| exit 1`. Checking only
# for `.error` lets an empty body or a timeout pass as success, which is how the
# first version of this script silently produced blank key material.
call() { # port, method, params, label
  local out err
  out=$(rpc "$1" "$2" "$3")
  if [ -z "$out" ]; then echo "FAIL $4: empty response" >&2; return 1; fi
  if ! echo "$out" | jq -e . >/dev/null 2>&1; then
    echo "FAIL $4: non-JSON response: ${out:0:120}" >&2; return 1
  fi
  err=$(echo "$out" | jq -r '.error.message // empty')
  if [ -n "$err" ]; then echo "FAIL $4: $err" >&2; return 1; fi
  if ! echo "$out" | jq -e '.result' >/dev/null 2>&1; then
    echo "FAIL $4: no result field" >&2; return 1
  fi
  echo "$out"
}

t0=$(date +%s)

echo "=== 1. create one wallet per party ==="
for i in 0 1 2; do
  r=$(rpc "${PORTS[$i]}" create_wallet \
      "{\"filename\":\"ms_${NAMES[$i]}\",\"password\":\"$PW\",\"language\":\"English\"}")
  if echo "$r" | jq -e '.error' >/dev/null 2>&1; then
    call "${PORTS[$i]}" open_wallet \
      "{\"filename\":\"ms_${NAMES[$i]}\",\"password\":\"$PW\"}" "open ${NAMES[$i]}" >/dev/null
    echo "  ${NAMES[$i]}: reused"
  else
    echo "  ${NAMES[$i]}: created"
  fi
done

echo
echo "=== 2. prepare_multisig ==="
declare -A INFO
for i in 0 1 2; do
  r=$(call "${PORTS[$i]}" prepare_multisig '{}' "prepare ${NAMES[$i]}") || exit 1
  INFO[$i]=$(echo "$r" | jq -r '.result.multisig_info')
  [ -n "${INFO[$i]}" ] || { echo "FAIL: empty prepare info for ${NAMES[$i]}" >&2; exit 1; }
  echo "  ${NAMES[$i]}: ${#INFO[$i]} chars"
done

peers_of() { # index -> JSON array of the other two infos
  local me=$1 out=()
  for j in 0 1 2; do [ "$j" = "$me" ] || out+=("\"${INFO[$j]}\""); done
  local IFS=,; echo "[${out[*]}]"
}

echo
echo "=== 3. make_multisig (threshold 2 of 3) ==="
declare -A NEXT
for i in 0 1 2; do
  r=$(call "${PORTS[$i]}" make_multisig \
      "{\"multisig_info\":$(peers_of $i),\"threshold\":2,\"password\":\"$PW\"}" \
      "make ${NAMES[$i]}") || exit 1
  NEXT[$i]=$(echo "$r" | jq -r '.result.multisig_info // empty')
  echo "  ${NAMES[$i]}: addr=$(echo "$r" | jq -r '.result.address' | cut -c1-20)... next=${#NEXT[$i]} chars"
done
for i in 0 1 2; do INFO[$i]="${NEXT[$i]}"; done

echo
echo "=== 4. exchange_multisig_keys until ready ==="
ROUND=1
while :; do
  DONE=1
  for i in 0 1 2; do
    st=$(call "${PORTS[$i]}" is_multisig '{}' "is_multisig ${NAMES[$i]}") || exit 1
    [ "$(echo "$st" | jq -r '.result.ready')" = "true" ] && continue
    DONE=0
  done
  [ "$DONE" = "1" ] && break

  for i in 0 1 2; do
    r=$(call "${PORTS[$i]}" exchange_multisig_keys \
        "{\"multisig_info\":$(peers_of $i),\"password\":\"$PW\"}" \
        "exchange r$ROUND ${NAMES[$i]}") || exit 1
    NEXT[$i]=$(echo "$r" | jq -r '.result.multisig_info // empty')
    echo "  round $ROUND ${NAMES[$i]}: addr=$(echo "$r" | jq -r '.result.address // "-"' | cut -c1-20)... next=${#NEXT[$i]} chars"
  done
  for i in 0 1 2; do INFO[$i]="${NEXT[$i]}"; done

  ROUND=$((ROUND+1))
  [ "$ROUND" -gt 6 ] && { echo "FAIL: no convergence in 6 rounds" >&2; exit 1; }
done

echo
echo "=== 5. final state ==="
declare -A ADDR
for i in 0 1 2; do
  st=$(call "${PORTS[$i]}" is_multisig '{}' "final ${NAMES[$i]}") || exit 1
  a=$(call "${PORTS[$i]}" get_address '{"account_index":0}' "addr ${NAMES[$i]}") || exit 1
  a=$(echo "$a" | jq -r '.result.address')
  ADDR[$i]=$a
  echo "  ${NAMES[$i]}: $(echo "$st" | jq -c '.result')"
done

t1=$(date +%s)
echo
echo "=== RESULT ==="
if [ "${ADDR[0]}" = "${ADDR[1]}" ] && [ "${ADDR[1]}" = "${ADDR[2]}" ]; then
  echo "  CONVERGED — all three parties derived the same address:"
  echo "  ${ADDR[0]}"
else
  echo "  MISMATCH — parties disagree:"
  for i in 0 1 2; do echo "    ${NAMES[$i]} ${ADDR[$i]}"; done
fi
echo "  key-exchange rounds after prepare: $ROUND"
echo "  total RPC calls: $CALLS"
echo "  wall clock: $((t1-t0))s"
