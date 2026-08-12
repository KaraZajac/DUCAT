#!/usr/bin/env bash
# DUCAT Monero spike — pre-split, take two.
#
# v1 was inconclusive through two faults of its own:
#   * it waited on `unlocked_balance != 0`, which fired when unrelated change
#     unlocked while the split outputs were still locked, so the measurement
#     never touched them;
#   * it counted outputs via `incoming_transfers transfer_type=available`,
#     which includes locked outputs and so reported spendable funds that
#     were not spendable.
#
# Both are fixed here. The wait is on unlocked *value* crossing the total the
# split created, which cannot be satisfied by leftover change. Payments are
# sized far below one split output so that unlocked value never becomes the
# binding constraint before unlocked output count does — v1's final refusal
# came with exactly the payment amount unlocked and no room for the fee, which
# measured fee headroom rather than output availability.
set -uo pipefail

PORT=28100
SELF=$(cat w_fund/fund.address.txt)
N=6                    # split into this many outputs
EACH=4000000000        # 0.004 XMR each
PAY=500000000          # 0.0005 XMR per payment — an eighth of one output
FEE_HEADROOM=300000000

rpc(){ curl -s -m 120 -X POST "http://127.0.0.1:$PORT/json_rpc" -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$1\",\"params\":$2}"; }
unlocked(){ rpc get_balance '{"account_index":0}' | jq -r '.result.unlocked_balance // 0'; }
bal(){ rpc get_balance '{"account_index":0}' | jq -c '.result | {balance, unlocked_balance, blocks_to_unlock}'; }

wait_for(){ # target unlocked value, label
  echo "  waiting until unlocked >= $1 ($2)..."
  for i in $(seq 1 100); do
    rpc refresh '{}' >/dev/null
    u=$(unlocked)
    [ $((i % 5)) -eq 1 ] && echo "    $(date +%H:%M:%S) $(bal)"
    if [ "$u" -ge "$1" ] 2>/dev/null; then echo "    reached: $u"; return 0; fi
    sleep 30
  done
  echo "  timed out" >&2; return 1
}

echo "=== start ==="; echo "  $(bal)"

echo
echo "=== 1. split into $N outputs of $EACH ==="
DEST=$(python3 -c "
import json
print(json.dumps([{'amount': $EACH, 'address': '$SELF'} for _ in range($N)]))
")
r=$(rpc transfer "{\"destinations\":$DEST,\"account_index\":0,\"priority\":1}")
if echo "$r" | jq -e '.error' >/dev/null 2>&1; then
  echo "  split REFUSED: $(echo "$r" | jq -r '.error.message')"; exit 1
fi
echo "  split ok: txid=$(echo "$r" | jq -r '.result.tx_hash' | cut -c1-16)... fee=$(echo "$r" | jq -r '.result.fee')"

# The split outputs total N*EACH. Waiting for that much unlocked value cannot
# be satisfied by leftover change, which is the flaw that broke v1.
TARGET=$((N * EACH))
wait_for "$TARGET" "the split outputs specifically" || exit 1

echo
echo "=== 2. drain: payments of $PAY until refused ==="
OK=0
for i in $(seq 1 15); do
  before=$(unlocked)
  r=$(rpc transfer "{\"destinations\":[{\"amount\":$PAY,\"address\":\"$SELF\"}],\"account_index\":0,\"priority\":1}")
  if echo "$r" | jq -e '.error' >/dev/null 2>&1; then
    msg=$(echo "$r" | jq -r '.error.message')
    echo "  payment $i REFUSED: $msg"
    echo "    unlocked at refusal: $before piconero"
    # Distinguish "out of outputs" from "out of value" — v1 could not.
    if [ "$before" -gt $((PAY + FEE_HEADROOM)) ] 2>/dev/null; then
      echo "    >>> value remained ($before) but the payment failed:"
      echo "        this is OUTPUT exhaustion, which is what §17.2 predicts"
    else
      echo "    >>> unlocked value was nearly gone: this measured fee headroom,"
      echo "        not output availability. Inconclusive again."
    fi
    break
  fi
  OK=$((OK+1))
  echo "  payment $i ok (unlocked before: $before)"
done

echo
echo "=== RESULT ==="
echo "  split into:              $N outputs"
echo "  consecutive payments:    $OK"
echo "  final: $(bal)"
