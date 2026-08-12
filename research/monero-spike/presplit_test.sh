#!/usr/bin/env bash
# DUCAT Monero spike — quantify §17.2's pre-split requirement.
#
# The claim under test: a float held as too few outputs cannot fund consecutive
# payments, because every spend locks its change for 10 blocks
# (CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE).
#
# Rather than demonstrate one failure, this measures the actual limit: make
# payments until one fails, and count. The prediction is that the count equals
# the number of unlocked outputs, before and after splitting.
set -uo pipefail

PORT=28100
SELF=$(cat w_fund/fund.address.txt)
PAY=1000000000        # 0.001 XMR per payment
SPLIT_N=10
SPLIT_EACH=5000000000 # 0.005 XMR per split output

rpc(){ curl -s -m 120 -X POST "http://127.0.0.1:$PORT/json_rpc" \
        -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$1\",\"params\":$2}"; }

bal(){ rpc get_balance '{"account_index":0}' | jq -c '.result | {balance, unlocked_balance, blocks_to_unlock}'; }
unlocked(){ rpc get_balance '{"account_index":0}' | jq -r '.result.unlocked_balance // 0'; }

# Count outputs the wallet considers spendable right now.
avail(){ rpc incoming_transfers '{"transfer_type":"available","account_index":0}' \
          | jq -r '[.result.transfers // [] | .[] | select(.spent==false)] | length'; }

wait_unlock(){
  echo "  waiting for unlock ($1)..."
  local i=0
  while [ "$(unlocked)" = "0" ]; do
    i=$((i+1))
    [ $((i % 5)) -eq 1 ] && echo "    $(date +%H:%M:%S) $(bal)"
    sleep 25
    rpc refresh '{}' >/dev/null
    [ $i -gt 160 ] && { echo "  gave up" >&2; return 1; }
  done
  echo "    unlocked: $(bal)"
}

# Spend repeatedly until refused. Returns the number that succeeded.
drain(){
  local n=0 i
  for i in $(seq 1 20); do
    local r
    r=$(rpc transfer "{\"destinations\":[{\"amount\":$PAY,\"address\":\"$SELF\"}],\"account_index\":0,\"priority\":1}")
    if echo "$r" | jq -e '.error' >/dev/null 2>&1; then
      echo "    payment $i REFUSED: $(echo "$r" | jq -r '.error.message')" >&2
      echo "    balance at refusal: $(bal)" >&2
      break
    fi
    n=$((n+1))
    echo "    payment $i ok  txid=$(echo "$r" | jq -r '.result.tx_hash' | cut -c1-16)..." >&2
  done
  echo "$n"
}

rpc refresh '{}' >/dev/null
echo "=== start ==="
echo "  $(bal)   spendable outputs: $(avail)"

wait_unlock "initial outputs" || exit 1

echo
echo "=== A. consecutive payments BEFORE splitting ==="
BEFORE_OUTPUTS=$(avail)
echo "  unlocked outputs: $BEFORE_OUTPUTS"
BEFORE=$(drain)
echo "  --> $BEFORE consecutive payments from $BEFORE_OUTPUTS unlocked output(s)"

echo
echo "=== B. pre-split into $SPLIT_N outputs ==="
wait_unlock "change from phase A" || exit 1
DEST=$(python3 -c "
import json,sys
print(json.dumps([{'amount': $SPLIT_EACH, 'address': '$SELF'} for _ in range($SPLIT_N)]))
")
r=$(rpc transfer "{\"destinations\":$DEST,\"account_index\":0,\"priority\":1}")
if echo "$r" | jq -e '.error' >/dev/null 2>&1; then
  echo "  split REFUSED: $(echo "$r" | jq -r '.error.message')"; exit 1
fi
echo "  split ok: txid=$(echo "$r" | jq -r '.result.tx_hash' | cut -c1-16)... fee=$(echo "$r" | jq -r '.result.fee')"

wait_unlock "split outputs" || exit 1

echo
echo "=== C. consecutive payments AFTER splitting ==="
AFTER_OUTPUTS=$(avail)
echo "  unlocked outputs: $AFTER_OUTPUTS"
AFTER=$(drain)
echo "  --> $AFTER consecutive payments from $AFTER_OUTPUTS unlocked output(s)"

echo
echo "=== RESULT ==="
printf "  before split: %s outputs -> %s consecutive payments\n" "$BEFORE_OUTPUTS" "$BEFORE"
printf "  after  split: %s outputs -> %s consecutive payments\n" "$AFTER_OUTPUTS" "$AFTER"
echo "  final: $(bal)"
