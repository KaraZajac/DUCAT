#!/usr/bin/env bash
# DUCAT Monero spike — validate §17.2's pre-split requirement.
#
# The claim under test: "A float held as a single output funds exactly one
# payment per lock interval: the second tap fails with a full balance showing
# on screen. This is not a corner case, it is the second ride."
#
# Sequence:
#   1. wait for the single received output to unlock
#   2. confirm the single-output failure: two payments back to back, second fails
#   3. pre-split into N outputs, wait for them to unlock
#   4. confirm the fix: N payments back to back all succeed
set -uo pipefail

PORT=28100
SELF=$(cat w_fund/fund.address.txt)
N=8

rpc(){ curl -s -m 120 -X POST "http://127.0.0.1:$PORT/json_rpc" \
        -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$1\",\"params\":$2}"; }

bal(){ rpc get_balance '{"account_index":0}' | jq -c '.result | {balance, unlocked_balance, blocks_to_unlock}'; }

unlocked(){ rpc get_balance '{"account_index":0}' | jq -r '.result.unlocked_balance'; }

outputs(){ # count of unlocked, spendable outputs
  rpc incoming_transfers '{"transfer_type":"available","account_index":0}' \
    | jq -r '[.result.transfers // [] | .[] | select(.spent==false)] | length'
}

wait_unlock(){ # label
  echo "  waiting for unlock ($1)..."
  local i=0
  while [ "$(unlocked)" = "0" ]; do
    i=$((i+1))
    [ $((i % 6)) -eq 1 ] && echo "    $(date +%H:%M:%S) $(bal)"
    sleep 20
    rpc refresh '{}' >/dev/null
    [ $i -gt 200 ] && { echo "  gave up waiting" >&2; return 1; }
  done
  echo "  unlocked: $(bal)"
}

echo "=== state at start ==="
rpc refresh '{}' >/dev/null
echo "  $(bal)"
echo "  spendable outputs: $(outputs)"

wait_unlock "initial output" || exit 1

echo
echo "=== 2. single-output behaviour: two payments back to back ==="
echo "  outputs available: $(outputs)"
for i in 1 2; do
  r=$(rpc transfer "{\"destinations\":[{\"amount\":1000000000,\"address\":\"$SELF\"}],\"account_index\":0,\"priority\":1,\"get_tx_key\":true}")
  if echo "$r" | jq -e '.error' >/dev/null; then
    echo "  payment $i: FAILED — $(echo "$r" | jq -r '.error.message')"
    echo "             balance at failure: $(bal)"
  else
    echo "  payment $i: ok  txid=$(echo "$r" | jq -r '.result.tx_hash' | cut -c1-16)... fee=$(echo "$r" | jq -r '.result.fee')"
  fi
done

echo
echo "=== 3. pre-split into $N outputs ==="
wait_unlock "post-payment change" || exit 1
DEST=$(python3 - "$SELF" "$N" <<'PY'
import sys, json
addr, n = sys.argv[1], int(sys.argv[2])
print(json.dumps([{"amount": 500000000, "address": addr} for _ in range(n)]))
PY
)
r=$(rpc transfer "{\"destinations\":$DEST,\"account_index\":0,\"priority\":1,\"get_tx_key\":true}")
if echo "$r" | jq -e '.error' >/dev/null; then
  echo "  split FAILED — $(echo "$r" | jq -r '.error.message')"
else
  echo "  split ok: txid=$(echo "$r" | jq -r '.result.tx_hash' | cut -c1-16)... fee=$(echo "$r" | jq -r '.result.fee')"
fi

wait_unlock "split outputs" || exit 1
echo "  spendable outputs after split: $(outputs)"

echo
echo "=== 4. with a pre-split float: consecutive payments ==="
OK=0; FAIL=0
for i in $(seq 1 4); do
  r=$(rpc transfer "{\"destinations\":[{\"amount\":200000000,\"address\":\"$SELF\"}],\"account_index\":0,\"priority\":1}")
  if echo "$r" | jq -e '.error' >/dev/null; then
    FAIL=$((FAIL+1)); echo "  payment $i: FAILED — $(echo "$r" | jq -r '.error.message')"
  else
    OK=$((OK+1)); echo "  payment $i: ok  txid=$(echo "$r" | jq -r '.result.tx_hash' | cut -c1-16)..."
  fi
done

echo
echo "=== RESULT ==="
echo "  consecutive payments from a pre-split float: $OK ok, $FAIL failed"
echo "  final: $(bal)"
echo "  outputs: $(outputs)"
