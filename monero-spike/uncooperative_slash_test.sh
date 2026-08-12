#!/usr/bin/env bash
# DUCAT Monero spike — can a bond be slashed WITHOUT the party being slashed?
#
# The previous test signed with arbiter + recovery, but had all three wallets
# export and import multisig key images first. That is not the slash scenario.
# A rider facing a slash does not help: they refuse, go offline, or uninstall.
#
# In Monero multisig, spending requires reconstructing key images from
# participants' partial key images. If M-of-N needs more than M participants to
# export, then a 2-of-3 bond cannot be slashed without the user's cooperation —
# and §17.2's deposit model collapses, because collateral you cannot seize over
# an objection is not collateral.
#
# This test never contacts the user wallet. Not once.
set -uo pipefail

USER_PORT=28088          # deliberately unused after the initial check
ARB=28089
REC=28090
PW=""
DEST=${1:-}
[ -n "$DEST" ] || { echo "usage: $0 <destination-address>" >&2; exit 1; }

rpc(){ curl -s -m 120 -X POST "http://127.0.0.1:$1/json_rpc" -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$2\",\"params\":$3}"; }

call(){
  local out err
  out=$(rpc "$1" "$2" "$3")
  [ -n "$out" ] || { echo "FAIL $4: empty" >&2; return 1; }
  err=$(echo "$out" | jq -r '.error.message // empty')
  [ -z "$err" ] || { echo "FAIL $4: $err" >&2; return 1; }
  echo "$out"
}

echo "=== 0. confirm the user wallet is deliberately excluded ==="
echo "  user wallet is on port $USER_PORT and will not be contacted again."

echo
echo "=== 1. open only arbiter and recovery ==="
for p in $ARB $REC; do
  n=$([ "$p" = "$ARB" ] && echo arbiter || echo recovery)
  call "$p" open_wallet "{\"filename\":\"ms_$n\",\"password\":\"$PW\"}" "open $n" >/dev/null || exit 1
  call "$p" refresh '{}' "refresh $n" >/dev/null || exit 1
  b=$(call "$p" get_balance '{"account_index":0}' "balance $n") || exit 1
  echo "  $n: $(echo "$b" | jq -c '.result | {balance, unlocked_balance, multisig_import_needed}')"
done

echo
echo "=== 2. wait for the change output to unlock ==="
for i in $(seq 1 80); do
  call "$ARB" refresh '{}' "refresh" >/dev/null || exit 1
  u=$(call "$ARB" get_balance '{"account_index":0}' "balance") || exit 1
  unl=$(echo "$u" | jq -r '.result.unlocked_balance')
  btu=$(echo "$u" | jq -r '.result.blocks_to_unlock')
  [ $((i % 4)) -eq 1 ] && echo "  $(date +%H:%M:%S) unlocked=$unl blocks_to_unlock=$btu"
  [ "$unl" != "0" ] && { echo "  spendable: $unl piconero"; break; }
  sleep 30
done

echo
echo "=== 3. export/import key images between ARBITER and RECOVERY ONLY ==="
A_INFO=$(call "$ARB" export_multisig_info '{}' "export arbiter" | jq -r '.result.info') || exit 1
R_INFO=$(call "$REC" export_multisig_info '{}' "export recovery" | jq -r '.result.info') || exit 1
echo "  arbiter exported  ${#A_INFO} chars"
echo "  recovery exported ${#R_INFO} chars"

r=$(call "$ARB" import_multisig_info "{\"info\":[\"$R_INFO\"]}" "import into arbiter") || exit 1
echo "  arbiter imported recovery's only: n_outputs=$(echo "$r" | jq -r '.result.n_outputs')"
r=$(call "$REC" import_multisig_info "{\"info\":[\"$A_INFO\"]}" "import into recovery") || exit 1
echo "  recovery imported arbiter's only: n_outputs=$(echo "$r" | jq -r '.result.n_outputs')"

echo
echo "=== 4. arbiter proposes, recovery co-signs, neither consults the user ==="
r=$(call "$ARB" transfer \
    "{\"destinations\":[{\"amount\":1000000000,\"address\":\"$DEST\"}],\"account_index\":0,\"priority\":1}" \
    "propose") || { echo; echo ">>> A SLASH REQUIRES THE SLASHED PARTY'S COOPERATION. This is fatal to §17.2." >&2; exit 1; }
TXSET=$(echo "$r" | jq -r '.result.multisig_txset')
echo "  proposed: multisig_txset ${#TXSET} chars"

r=$(call "$REC" sign_multisig "{\"tx_data_hex\":\"$TXSET\"}" "cosign") || exit 1
SIGNED=$(echo "$r" | jq -r '.result.tx_data_hex')
echo "  co-signed: ${#SIGNED} chars"

r=$(call "$REC" submit_multisig "{\"tx_data_hex\":\"$SIGNED\"}" "submit") || exit 1
echo "  submitted: $(echo "$r" | jq -c '.result.tx_hash_list')"

echo
echo "=== RESULT ==="
echo "  The bond was seized by arbiter + recovery with the user wallet never"
echo "  contacted — not for signing, and not for key images. Collateral that"
echo "  can be taken over the holder's objection is collateral. §17.2 holds."
