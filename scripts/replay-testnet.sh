#!/usr/bin/env bash
# Replay a shrunk Breakwater sequence on a Stellar network with the Stellar CLI.
#
# Input is the JSON written by `breakwater::Exporter` (a RunReport). Only the
# policy-bypass suite is supported: the three agent `transfer` steps are
# submitted as real transactions through the smart account's `__check_auth`.
#
# Usage:
#   scripts/replay-testnet.sh <report.json> [options]
#
# Options:
#   --network <name>       Stellar CLI network name (default: testnet)
#   --source <identity>    identity that pays fees and acts as asset issuer (default: breakwater-dev)
#   --merchant <identity>  identity that receives the payments (default: breakwater-recipient)
#   --agent <identity>     identity whose Ed25519 key is the agent signer (default: breakwater-agent)
#   --owner <identity>     identity whose Ed25519 key is the owner signer (default: breakwater-owner)
#   --state <file>         JSON state file (default: breakwater-out/replay-<network>.json)
#   --until-step N         stop after step N (0-based, inclusive)
#   --resume-from N        skip setup and steps before N (state file must exist)
#   --dry-run              print every command instead of running it
#
# Secrets never appear on a command line or in output: signer seeds are read by
# `stellar keys secret` and passed to the helper through the environment.
#
# Wasm inputs (env, defaults are what `stellar contract build --package` writes):
#   ACCOUNT_WASM   example-policy-bypass          -> DemoAccount
#   OWNER_WASM     example-owner-account          -> OwnerAccount
#   POLICY_WASM    example-spending-limit-policy  -> SpendingLimitPolicy
#   VERIFIER_WASM  example-ed25519-verifier       -> Ed25519Verifier

set -euo pipefail

usage() { sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'; }

REPORT=""
NETWORK="testnet"
SOURCE="breakwater-dev"
MERCHANT="breakwater-recipient"
AGENT="breakwater-agent"
OWNER="breakwater-owner"
STATE=""
UNTIL_STEP=""
RESUME_FROM=""
DRY_RUN=0

while [ $# -gt 0 ]; do
  case "$1" in
    --network) NETWORK="$2"; shift 2 ;;
    --source) SOURCE="$2"; shift 2 ;;
    --merchant) MERCHANT="$2"; shift 2 ;;
    --agent) AGENT="$2"; shift 2 ;;
    --owner) OWNER="$2"; shift 2 ;;
    --state) STATE="$2"; shift 2 ;;
    --until-step) UNTIL_STEP="$2"; shift 2 ;;
    --resume-from) RESUME_FROM="$2"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    -*) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    *) REPORT="$1"; shift ;;
  esac
done

[ -n "$REPORT" ] || { echo "error: report.json is required" >&2; usage >&2; exit 2; }
[ -f "$REPORT" ] || { echo "error: $REPORT not found" >&2; exit 2; }
command -v jq >/dev/null || { echo "error: jq is required" >&2; exit 2; }
command -v stellar >/dev/null || { echo "error: stellar CLI is required" >&2; exit 2; }
STATE="${STATE:-breakwater-out/replay-${NETWORK}.json}"
mkdir -p "$(dirname "$STATE")"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASM_DIR="$ROOT/target/wasm32v1-none/release"
ACCOUNT_WASM="${ACCOUNT_WASM:-$WASM_DIR/example_policy_bypass.wasm}"
OWNER_WASM="${OWNER_WASM:-$WASM_DIR/example_owner_account.wasm}"
POLICY_WASM="${POLICY_WASM:-$WASM_DIR/example_spending_limit_policy.wasm}"
VERIFIER_WASM="${VERIFIER_WASM:-$WASM_DIR/example_ed25519_verifier.wasm}"

USDC=10000000           # 1 USDC in stroops
PER_CALL_CAP_USDC=50
DAILY_CAP_USDC=100
BUGGY_DAILY_WINDOW=1728 # the mis-sized "daily" window the demo installs
MERCHANT_ADDR="$(stellar keys address "$MERCHANT")"
SOURCE_ADDR="$(stellar keys address "$SOURCE")"

log() { printf '%s\n' "$*" >&2; }
run() {
  if [ "$DRY_RUN" = 1 ]; then
    printf '+ %s\n' "$*"
    return 0
  fi
  "$@"
}
# run and capture stdout (dry-run returns a placeholder)
cap() {
  if [ "$DRY_RUN" = 1 ]; then
    printf '+ %s\n' "$*" >&2
    printf 'DRYRUN'
    return 0
  fi
  "$@"
}

state_get() { [ -f "$STATE" ] && jq -r "$1 // empty" "$STATE" || true; }
state_set() { # key value
  local tmp; tmp="$(mktemp)"
  if [ -f "$STATE" ]; then jq --arg k "$1" --arg v "$2" '.[$k] = $v' "$STATE" > "$tmp"; else jq -n --arg k "$1" --arg v "$2" '{($k): $v}' > "$tmp"; fi
  mv "$tmp" "$STATE"
}

# hex of an identity's Ed25519 public key (External signer key_data, BytesN<32> args)
pubkey_hex() {
  if [ "$DRY_RUN" = 1 ]; then printf 'PUBKEY_HEX_%s' "$1"; return; fi
  BREAKWATER_MODE=pubkey-hex BREAKWATER_PUBLIC_KEY="$(stellar keys address "$1")" \
    cargo run -q -p breakwater-pack-policy-bypass --bin oz-auth-entry
}

latest_ledger() {
  if [ "$DRY_RUN" = 1 ]; then echo 0; return; fi
  stellar ledger latest --network "$NETWORK" 2>/dev/null | awk '/Sequence:/ {print $2}'
}

# ---------------------------------------------------------------- setup ----
setup() {
  log "== setup on $NETWORK (source $SOURCE = $SOURCE_ADDR)"
  log "-- build contract wasm files"
  for pkg in example-policy-bypass example-owner-account example-spending-limit-policy example-ed25519-verifier; do
    run stellar contract build --package "$pkg" >/dev/null
  done
  for w in ACCOUNT_WASM OWNER_WASM POLICY_WASM VERIFIER_WASM; do
    [ "$DRY_RUN" = 1 ] || [ -f "${!w}" ] || { log "error: ${!w} not found ($w)"; exit 2; }
  done

  log "-- deploy helper contracts"
  local verifier policy_per_call policy_daily owner_account account token
  verifier="$(cap stellar contract deploy --network "$NETWORK" --source "$SOURCE" --wasm "$VERIFIER_WASM")"
  policy_per_call="$(cap stellar contract deploy --network "$NETWORK" --source "$SOURCE" --wasm "$POLICY_WASM")"
  policy_daily="$(cap stellar contract deploy --network "$NETWORK" --source "$SOURCE" --wasm "$POLICY_WASM")"
  # The owner account verifies an Ed25519 signature: its constructor takes the owner's public key.
  local owner_pk; owner_pk="$(pubkey_hex "$OWNER")"
  owner_account="$(cap stellar contract deploy --network "$NETWORK" --source "$SOURCE" --wasm "$OWNER_WASM" -- --public_key "$owner_pk")"

  log "-- deploy the test asset (USDC issued by $SOURCE) and let the merchant hold it"
  token="$(cap stellar contract asset deploy --network "$NETWORK" --source "$SOURCE" --asset "USDC:$SOURCE_ADDR")"
  run stellar tx new change-trust --network "$NETWORK" --source "$MERCHANT" --line "USDC:$SOURCE_ADDR"

  log "-- deploy DemoAccount with its two context rules (constructor InitialRule list)"
  # Same configuration as examples/policy-bypass/tests/suite.rs: rule 0 owner
  # (Default, Delegated owner account), rule 1 agent (CallContract(token),
  # External ed25519 key, 50 USDC per ledger and 100 USDC per 1 728 ledgers).
  local rules
  rules="$(jq -nc \
    --arg owner "$owner_account" --arg verifier "$verifier" --arg agent_pk_hex "$(pubkey_hex "$AGENT")" \
    --arg token "$token" --arg p1 "$policy_per_call" --arg p2 "$policy_daily" \
    --arg cap "$((PER_CALL_CAP_USDC * USDC))" --arg daily "$((DAILY_CAP_USDC * USDC))" --arg window "$BUGGY_DAILY_WINDOW" '
    [
      { context_type: "Default", name: "owner", valid_until: null,
        signers: [ { Delegated: $owner } ], policies: {}, spending_limits: [] },
      { context_type: { CallContract: $token }, name: "agent", valid_until: null,
        signers: [ { External: [ $verifier, $agent_pk_hex ] } ], policies: {},
        spending_limits: [ { policy: $p1, spending_limit: $cap, period_ledgers: 1 },
                           { policy: $p2, spending_limit: $daily, period_ledgers: ($window|tonumber) } ] }
    ]')"
  account="$(cap stellar contract deploy --network "$NETWORK" --source "$SOURCE" --wasm "$ACCOUNT_WASM" -- --rules "$rules")"

  log "-- mint 10 000 USDC to the smart account"
  run stellar contract invoke --network "$NETWORK" --source "$SOURCE" --id "$token" -- mint --to "$account" --amount "$((10000 * USDC))"

  state_set network "$NETWORK"
  state_set account "$account"
  state_set token "$token"
  state_set verifier "$verifier"
  state_set policy_per_call "$policy_per_call"
  state_set policy_daily "$policy_daily"
  state_set owner_account "$owner_account"
  state_set agent_rule "1"
  state_set completed_steps "0"
  state_set current_ledger "$(latest_ledger)"
  log "state written to $STATE"
}

# ------------------------------------------------------------- one step ----
# Submits `usdc.transfer(account, merchant, amount)` authorized by the agent:
#  1. build the transaction (`--build-only` is not simulated) and simulate it in
#     recording mode, which records an auth entry for the account with the nonce
#     set, the expiration unset and a void signature,
#  2. take nonce / root invocation from that entry and pick an expiration ledger,
#  3. have oz-auth-entry sign the OpenZeppelin AuthPayload for it,
#  4. splice the signed entry back, simulate again (enforce mode, so the
#     footprint and fees include __check_auth and the policies), sign, send.
agent_transfer() { # amount_usdc
  local amount=$(( $1 * USDC ))
  local account token
  account="$(state_get .account)"; token="$(state_get .token)"
  log "-- transfer $1 USDC from $account to $MERCHANT_ADDR"

  local built decoded nonce expiration invocation_json invocation_xdr signed_entries patched final
  built="$(cap stellar contract invoke --network "$NETWORK" --source "$SOURCE" --id "$token" --build-only -- transfer --from "$account" --to "$MERCHANT_ADDR" --amount "$amount")"
  local recorded
  recorded="$(cap sh -c "printf '%s' '$built' | stellar tx simulate --network '$NETWORK' --source-account '$SOURCE'")"
  decoded="$(cap sh -c "printf '%s' '$recorded' | stellar tx decode")"
  if [ "$DRY_RUN" = 1 ]; then
    log "   (then: extract .tx.tx.operations[0].body.invoke_host_function.auth[0] nonce, signature_expiration_ledger, root_invocation;"
    log "    stellar xdr encode --type SorobanAuthorizedInvocation; oz-auth-entry; stellar xdr decode --type SorobanAuthorizationEntry;"
    log "    jq-splice into the envelope; stellar tx encode | stellar tx sign --sign-with-key $SOURCE | stellar tx send)"
    return 0
  fi
  nonce="$(printf '%s' "$decoded" | jq -r '.tx.tx.operations[0].body.invoke_host_function.auth[0].credentials.address.nonce')"
  expiration="$(printf '%s' "$decoded" | jq -r '.tx.tx.operations[0].body.invoke_host_function.auth[0].credentials.address.signature_expiration_ledger')"
  # Recording-mode simulation may leave nonce / expiration unset; pick our own then.
  if [ -z "$nonce" ] || [ "$nonce" = "null" ] || [ "$nonce" = "0" ]; then nonce="$(( $(date +%s) ))"; fi
  if [ -z "$expiration" ] || [ "$expiration" = "null" ] || [ "$expiration" = "0" ]; then expiration=$(( $(latest_ledger) + 200 )); fi
  log "   auth entry: nonce=$nonce expiration_ledger=$expiration"
  invocation_json="$(printf '%s' "$decoded" | jq -c '.tx.tx.operations[0].body.invoke_host_function.auth[0].root_invocation')"
  invocation_xdr="$(printf '%s' "$invocation_json" | stellar xdr encode --type SorobanAuthorizedInvocation)"

  signed_entries="$(BREAKWATER_NETWORK_PASSPHRASE="$(stellar network ls -l | awk -v n="$NETWORK" '$0 ~ "Name: "n {f=1} f && /Network passphrase:/ {sub(/.*passphrase: /, ""); print; exit}')" \
    BREAKWATER_ACCOUNT="$account" BREAKWATER_RULE_ID="$(state_get .agent_rule)" \
    BREAKWATER_NONCE="$nonce" BREAKWATER_EXPIRATION_LEDGER="$expiration" \
    BREAKWATER_INVOCATION_XDR="$invocation_xdr" BREAKWATER_VERIFIER="$(state_get .verifier)" \
    BREAKWATER_SIGNER_SECRET="$(stellar keys secret "$AGENT")" \
    cargo run -q -p breakwater-pack-policy-bypass --bin oz-auth-entry)"
  local entry_json
  entry_json="$(printf '%s' "$signed_entries" | head -1 | stellar xdr decode --type SorobanAuthorizationEntry --output json)"
  patched="$(printf '%s' "$decoded" | jq --argjson e "$entry_json" '.tx.tx.operations[0].body.invoke_host_function.auth = [$e]' | stellar tx encode)"
  # Re-simulate with the signed entry in place: recording-mode simulation did not
  # run __check_auth, so the footprint and fees must be recomputed (enforce mode).
  local assembled
  assembled="$(printf '%s' "$patched" | stellar tx simulate --network "$NETWORK" --source-account "$SOURCE")"
  local signed hash
  signed="$(printf '%s' "$assembled" | stellar tx sign --network "$NETWORK" --sign-with-key "$SOURCE")"
  hash="$(printf '%s' "$signed" | stellar tx hash)"
  final="$(printf '%s' "$signed" | stellar tx send --network "$NETWORK")"
  log "   tx $hash -> $(printf '%s' "$final" | jq -r '.status // "sent"' 2>/dev/null || echo sent)"
  log "   https://stellar.expert/explorer/testnet/tx/$hash"
  state_set "tx_step_${CURRENT_STEP}" "$hash"
}

wait_for_ledger() { # target
  local target="$1" now
  now="$(latest_ledger)"
  while [ "$now" -lt "$target" ]; do
    local remaining=$(( target - now ))
    log "   ledger $now, target $target, ~$(( remaining * 5 / 60 )) min to wait (at ~5 s per ledger)"
    sleep $(( remaining > 120 ? 600 : remaining * 5 ))
    now="$(latest_ledger)"
  done
}

# ------------------------------------------------------------- main -----
if [ -z "$RESUME_FROM" ]; then
  setup
  RESUME_FROM=0
else
  [ -f "$STATE" ] || { echo "error: --resume-from needs the state file $STATE" >&2; exit 2; }
fi

STEPS="$(jq -c '.outcome.log.steps[]' "$REPORT")"
INDEX=0
START_LEDGER="$(latest_ledger)"
LAST_STEP_LEDGER="$(state_get .last_step_ledger)"; LAST_STEP_LEDGER="${LAST_STEP_LEDGER:-$START_LEDGER}"
printf '%s\n' "$STEPS" | while IFS= read -r step; do
  handler="$(printf '%s' "$step" | jq -r .handler)"
  actor="$(printf '%s' "$step" | jq -r .actor.name)"
  gap="$(printf '%s' "$step" | jq -r .gap.ledgers)"
  if [ "$INDEX" -lt "$RESUME_FROM" ]; then INDEX=$((INDEX + 1)); continue; fi
  if [ -n "$UNTIL_STEP" ] && [ "$INDEX" -gt "$UNTIL_STEP" ]; then break; fi
  if [ "$INDEX" -gt 0 ] && [ "$gap" -gt 1 ]; then
    target=$(( LAST_STEP_LEDGER + gap ))
    log "== step $INDEX waits $gap ledgers (until ledger $target)"
    [ "$DRY_RUN" = 1 ] || wait_for_ledger "$target"
  fi
  log "== step $INDEX: $actor $handler $(printf '%s' "$step" | jq -c .args)"
  CURRENT_STEP="$INDEX"
  case "$handler" in
    transfer) agent_transfer "$(printf '%s' "$step" | jq -r .args.amount_usdc)" ;;
    *) log "   handler $handler is not supported by the replay; skipping" ;;
  esac
  LAST_STEP_LEDGER="$(latest_ledger)"
  state_set completed_steps "$((INDEX + 1))"
  state_set "ledger_step_${INDEX}" "$LAST_STEP_LEDGER"
  state_set last_step_ledger "$LAST_STEP_LEDGER"
  state_set current_ledger "$LAST_STEP_LEDGER"
  INDEX=$((INDEX + 1))
done
log "done; state in $STATE"
