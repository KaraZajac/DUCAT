#!/usr/bin/env bash
# Resume a half-built 2-of-3 multisig from the exchange_multisig_keys stage.
#
# Needed because make_multisig resets enable-multisig-experimental to 0, so the
# flag must be re-set out-of-band, via monero-wallet-cli, on every party,
# *between* two steps of a single setup ceremony.
set -uo pipefail

PORTS=(28088 28089 28090)
NAMES=(user arbiter recovery)
PW=""

call() {
  local out err
  out=$(curl -s -m 60 -X POST "http://127.0.0.1:$1/json_rpc" \
        -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$2\",\"params\":$3}")
  [ -n "$out" ] || { echo "FAIL $4: empty" >&2; return 1; }
  err=$(echo "$out" | jq -r '.error.message // empty')
  [ -z "$err" ] || { echo "FAIL $4: $err" >&2; return 1; }
  echo "$out"
}

declare -A INFO
echo "=== current state ==="
for i in 0 1 2; do
  st=$(call "${PORTS[$i]}" is_multisig '{}' "state ${NAMES[$i]}") || exit 1
  echo "  ${NAMES[$i]}: $(echo "$st" | jq -c '.result')"
done

# Re-derive the round-1 exchange material each party must broadcast. After
# make_multisig, that is what export_multisig_info / the prior make output
# carried; wallet-rpc re-emits it from exchange_multisig_keys itself.
echo
echo "=== exchange rounds ==="
ROUND=1
# Seed with each party's post-make info, obtained by calling exchange with an
# empty peer set is not valid, so we take it from make_multisig's stored output.
if [ ! -f make_info.txt ]; then
  echo "make_info.txt not found — rerun multisig_test.sh to regenerate" >&2
  exit 1
fi
i=0
while read -r line; do INFO[$i]="$line"; i=$((i+1)); done < make_info.txt

peers_of() {
  local me=$1 out=()
  for j in 0 1 2; do [ "$j" = "$me" ] || out+=("\"${INFO[$j]}\""); done
  local IFS=,; echo "[${out[*]}]"
}

while :; do
  DONE=1
  for i in 0 1 2; do
    st=$(call "${PORTS[$i]}" is_multisig '{}' "state ${NAMES[$i]}") || exit 1
    [ "$(echo "$st" | jq -r '.result.ready')" = "true" ] || DONE=0
  done
  [ "$DONE" = "1" ] && break

  declare -A NEXT
  for i in 0 1 2; do
    r=$(call "${PORTS[$i]}" exchange_multisig_keys \
        "{\"multisig_info\":$(peers_of $i),\"password\":\"$PW\"}" \
        "exchange r$ROUND ${NAMES[$i]}") || exit 1
    NEXT[$i]=$(echo "$r" | jq -r '.result.multisig_info // empty')
    echo "  r$ROUND ${NAMES[$i]}: addr=$(echo "$r" | jq -r '.result.address // "-"' | cut -c1-22)... next=${#NEXT[$i]}"
  done
  for i in 0 1 2; do INFO[$i]="${NEXT[$i]}"; done
  ROUND=$((ROUND+1))
  [ "$ROUND" -gt 6 ] && { echo "FAIL: no convergence" >&2; exit 1; }
done

echo
echo "=== final ==="
declare -A ADDR
for i in 0 1 2; do
  st=$(call "${PORTS[$i]}" is_multisig '{}' "final ${NAMES[$i]}") || exit 1
  a=$(call "${PORTS[$i]}" get_address '{"account_index":0}' "addr ${NAMES[$i]}") || exit 1
  ADDR[$i]=$(echo "$a" | jq -r '.result.address')
  echo "  ${NAMES[$i]}: $(echo "$st" | jq -c '.result')"
done
if [ "${ADDR[0]}" = "${ADDR[1]}" ] && [ "${ADDR[1]}" = "${ADDR[2]}" ]; then
  echo "  CONVERGED: ${ADDR[0]}"
else
  echo "  MISMATCH"; for i in 0 1 2; do echo "    ${NAMES[$i]} ${ADDR[$i]}"; done
fi
echo "  exchange rounds: $((ROUND-1))"
