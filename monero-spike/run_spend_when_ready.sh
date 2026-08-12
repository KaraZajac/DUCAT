#!/usr/bin/env bash
# Wait for the bond multisig to hold unlocked funds, then attempt the slash spend.
set -uo pipefail
DEST=$(cat w_fund/fund.address.txt)
rpc(){ curl -s -m 120 -X POST "http://127.0.0.1:$1/json_rpc" -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"method\":\"$2\",\"params\":$3}"; }

echo "waiting for the bond to confirm and unlock..."
for i in $(seq 1 90); do
  for p in 28088 28089 28090; do rpc $p refresh '{}' >/dev/null; done
  b=$(rpc 28088 get_balance '{"account_index":0}')
  bal=$(echo "$b" | jq -r '.result.balance // 0')
  unl=$(echo "$b" | jq -r '.result.unlocked_balance // 0')
  btu=$(echo "$b" | jq -r '.result.blocks_to_unlock // "?"')
  [ $((i % 4)) -eq 1 ] && echo "  $(date +%H:%M:%S) balance=$bal unlocked=$unl blocks_to_unlock=$btu"
  if [ "$unl" != "0" ] && [ -n "$unl" ]; then
    echo "  bond is spendable: $unl piconero"
    break
  fi
  sleep 30
done

echo
echo "########## MULTISIG SPEND TEST ##########"
./multisig_spend_test.sh "$DEST"
