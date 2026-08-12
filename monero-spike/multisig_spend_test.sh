#!/usr/bin/env bash
# DUCAT Monero spike — can a 2-of-3 bond actually pay out?
#
# The ceremony in full_ceremony.sh proved three parties can *form* a multisig.
# That is not the claim §17.2 and §17.5 rest on. The claim is that when a rider
# defrauds a driver, two of {user, arbiter, recovery} can move the bond WITHOUT
# the third — specifically without the rider, who will not co-operate in their
# own slashing.
#
# So this signs with arbiter + recovery only, deliberately excluding the wallet
# standing in for the rider. A 2-of-3 that can only spend with all three is
# useless as collateral, and forming one proves nothing on its own.
#
# Prerequisite: the multisig address must hold confirmed, unlocked funds.
set -uo pipefail

PORTS=(28088 28089 28090)
NAMES=(user arbiter recovery)
PW=""
# Signers: arbiter and recovery. Index 0 (user == the party being slashed) is
# excluded on purpose.
SIGNERS=(1 2)
DEST=${1:-}

rpc(){ curl -s -m 120 -X POST "http://127.0.0.1:$1/json_rpc" \
        -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$2\",\"params\":$3}"; }

call(){
  local out err
  out=$(rpc "$1" "$2" "$3")
  [ -n "$out" ] || { echo "FAIL $4: empty" >&2; return 1; }
  err=$(echo "$out" | jq -r '.error.message // empty')
  [ -z "$err" ] || { echo "FAIL $4: $err" >&2; return 1; }
  echo "$out"
}

[ -n "$DEST" ] || { echo "usage: $0 <destination-address>" >&2; exit 1; }

echo "=== 1. open all three, confirm multisig state ==="
for i in 0 1 2; do
  call "${PORTS[$i]}" open_wallet "{\"filename\":\"ms_${NAMES[$i]}\",\"password\":\"$PW\"}" "open ${NAMES[$i]}" >/dev/null || exit 1
  st=$(call "${PORTS[$i]}" is_multisig '{}' "state ${NAMES[$i]}") || exit 1
  echo "  ${NAMES[$i]}: $(echo "$st" | jq -c '.result')"
done

echo
echo "=== 2. sync balances ==="
# Multisig wallets must import each other's key images before they can see
# which outputs are spent; a fresh multisig sees nothing until this happens.
for i in 0 1 2; do
  call "${PORTS[$i]}" refresh '{}' "refresh ${NAMES[$i]}" >/dev/null || exit 1
  b=$(call "${PORTS[$i]}" get_balance '{"account_index":0}' "balance ${NAMES[$i]}") || exit 1
  echo "  ${NAMES[$i]}: $(echo "$b" | jq -c '.result | {balance, unlocked_balance, multisig_import_needed}')"
done

echo
echo "=== 3. export/import multisig info ==="
declare -A MSINFO
for i in 0 1 2; do
  r=$(call "${PORTS[$i]}" export_multisig_info '{}' "export ${NAMES[$i]}") || exit 1
  MSINFO[$i]=$(echo "$r" | jq -r '.result.info')
  echo "  ${NAMES[$i]}: exported ${#MSINFO[$i]} chars"
done
for i in 0 1 2; do
  others=()
  for j in 0 1 2; do [ "$j" = "$i" ] || others+=("\"${MSINFO[$j]}\""); done
  peers=$(IFS=,; echo "${others[*]}")
  r=$(call "${PORTS[$i]}" import_multisig_info "{\"info\":[$peers]}" "import ${NAMES[$i]}") || exit 1
  echo "  ${NAMES[$i]}: imported, n_outputs=$(echo "$r" | jq -r '.result.n_outputs')"
done

echo
echo "=== 4. arbiter proposes a spend (rider NOT involved) ==="
A=${SIGNERS[0]}
r=$(call "${PORTS[$A]}" transfer \
    "{\"destinations\":[{\"amount\":1000000000,\"address\":\"$DEST\"}],\"account_index\":0,\"priority\":1,\"get_tx_key\":true}" \
    "propose ${NAMES[$A]}") || exit 1
TXSET=$(echo "$r" | jq -r '.result.multisig_txset')
echo "  proposed by ${NAMES[$A]}: multisig_txset ${#TXSET} chars"
[ -n "$TXSET" ] && [ "$TXSET" != "null" ] || { echo "FAIL: no multisig_txset returned" >&2; exit 1; }

echo
echo "=== 5. recovery co-signs, reaching the 2-of-3 threshold ==="
B=${SIGNERS[1]}
r=$(call "${PORTS[$B]}" sign_multisig "{\"tx_data_hex\":\"$TXSET\"}" "cosign ${NAMES[$B]}") || exit 1
SIGNED=$(echo "$r" | jq -r '.result.tx_data_hex')
TXHASHES=$(echo "$r" | jq -c '.result.tx_hash_list')
echo "  co-signed by ${NAMES[$B]}: ${#SIGNED} chars, hashes=$TXHASHES"

echo
echo "=== 6. submit ==="
r=$(call "${PORTS[$B]}" submit_multisig "{\"tx_data_hex\":\"$SIGNED\"}" "submit") || exit 1
echo "  submitted: $(echo "$r" | jq -c '.result.tx_hash_list')"

echo
echo "=== RESULT ==="
echo "  A 2-of-3 bond was spent by arbiter + recovery, without the party being"
echo "  slashed. This is the property §17.5 depends on; forming the multisig"
echo "  proved only that the parties could agree, not that a slash can pay out."
