# invoke() → api() Migration Reference

Every `invoke("command", args)` call maps to an `api(path, body)` call.
Session token is sent automatically by `api()` via `X-Session-Token` header.

## Auth (`src/App.tsx`, auth components)

| Old invoke | New api call | Notes |
|---|---|---|
| `invoke("is_setup")` | `apiGet("/auth/status")` → `.setup` field | |
| `invoke("is_unlocked")` | `apiGet("/auth/status")` → `!.locked` field | |
| `invoke("setup", {pin, passphrase})` | `apiPost("/auth/setup", {pin, passphrase})` | Returns `{session_token, recovery_json}` |
| `invoke("login", {pin})` | `apiPost("/auth/login", {pin})` | Returns `{session_token}` |
| `invoke("lock")` | `apiPost("/auth/lock", {})` | Also call `clearSessionToken()` |
| `invoke("recover", {recovery_file_contents, passphrase, new_pin})` | `apiPost("/auth/recover", {recovery_json, passphrase, new_pin})` | |
| `invoke("change_pin", {current_pin, new_pin})` | `apiPost("/auth/change-pin", {current_pin, new_pin})` | |

## Persons / Portfolios / Accounts

| Old invoke | New api call |
|---|---|
| `invoke("get_persons")` | `apiGet("/persons")` |
| `invoke("create_person", {input})` | `apiPost("/persons", input)` |
| `invoke("update_person", {person_id, input})` | `apiPatch(\`/persons/${person_id}\`, input)` |
| `invoke("get_portfolios")` | `apiGet("/portfolios")` |
| `invoke("create_portfolio", {input})` | `apiPost("/portfolios", input)` |
| `invoke("rename_portfolio", {portfolio_id, name})` | `apiPatch(\`/portfolios/${portfolio_id}/rename\`, {name})` |
| `invoke("delete_portfolio", {portfolio_id})` | `apiDel(\`/portfolios/${portfolio_id}\`)` |
| `invoke("get_accounts", {portfolio_id})` | `apiGet(\`/accounts?portfolio_id=${portfolio_id}\`)` |
| `invoke("create_account", {input})` | `apiPost("/accounts", input)` |
| `invoke("rename_account", {account_id, name})` | `apiPatch(\`/accounts/${account_id}/rename\`, {name})` |
| `invoke("delete_account", {account_id})` | `apiDel(\`/accounts/${account_id}\`)` |

## Instruments

| Old invoke | New api call |
|---|---|
| `invoke("search_instruments", {query})` | `apiGet(\`/instruments/search?q=${encodeURIComponent(query)}\`)` |
| `invoke("get_instrument_types")` | `apiGet("/instruments/types")` |
| `invoke("create_instrument", {input})` | `apiPost("/instruments", input)` |
| `invoke("update_pending_instrument", {pending_id, name, metadata})` | `apiPatch(\`/instruments/pending/${pending_id}\`, {name, metadata})` |

## Transactions

| Old invoke | New api call |
|---|---|
| `invoke("get_transactions", {filter})` | `apiPost("/transactions/list", filter)` |
| `invoke("get_transactions_count", {filter})` | `apiPost("/transactions/count", filter)` → `.count` |
| `invoke("get_flagged_count", {account_ids})` | `apiPost("/transactions/flagged-count", {account_ids})` → `.count` |
| `invoke("get_import_batch", {batch_id})` | `apiGet(\`/transactions/batch/${batch_id}\`)` |
| `invoke("create_transaction", {input})` | `apiPost("/transactions", input)` |
| `invoke("update_transaction", {input})` | `apiPatch("/transactions", input)` |
| `invoke("delete_transaction", {txn_id})` | `apiDel(\`/transactions/${txn_id}\`)` |
| `invoke("dismiss_transaction_flag", {txn_id})` | `apiPost(\`/transactions/${txn_id}/dismiss-flag\`, {})` |
| `invoke("re_evaluate_flags", {account_id})` | `apiPost("/transactions/re-evaluate-flags", {account_id})` |
| `invoke("transfer_holding", {input})` | `apiPost("/transactions/transfer", input)` |
| `invoke("create_split", {input})` | `apiPost("/transactions/split", input)` |

## Holdings

| Old invoke | New api call |
|---|---|
| `invoke("get_holdings", {account_ids, portfolio_ids, asset_classes})` | `apiPost("/holdings", {account_ids, portfolio_ids, asset_classes})` |
| `invoke("get_portfolio_summary", {account_ids, portfolio_ids, asset_classes})` | `apiPost("/holdings/summary", {account_ids, portfolio_ids, asset_classes})` |

## Reports

| Old invoke | New api call |
|---|---|
| `invoke("get_capital_gains", {fy, account_ids})` | `apiPost("/reports/capital-gains", {fy, account_ids})` |
| `invoke("get_income", {fy, account_ids})` | `apiPost("/reports/income", {fy, account_ids})` |
| `invoke("export_tax_report", {fy, path})` | `apiPost("/reports/export-tax", {fy, dest_path: path})` |
| `invoke("get_charges", {account_ids})` | `apiPost("/charges/list", {account_ids})` |
| `invoke("create_charge", {input})` | `apiPost("/charges", input)` |
| `invoke("update_charge", {charge_id, input})` | `apiPut(\`/charges/${charge_id}\`, input)` |
| `invoke("delete_charge", {charge_id})` | `apiDel(\`/charges/${charge_id}\`)` |

## Tax

| Old invoke | New api call |
|---|---|
| `invoke("get_tax_entries", {person_id, fy})` | `apiPost("/tax/list", {person_id, fy})` |
| `invoke("create_tax_entry", {input})` | `apiPost("/tax", input)` |
| `invoke("delete_tax_entry", {entry_id})` | `apiDel(\`/tax/${entry_id}\`)` |

## Settings

| Old invoke | New api call |
|---|---|
| `invoke("get_setting", {key})` | `apiGet(\`/settings/${key}\`)` → `.value` |
| `invoke("set_setting", {key, value})` | `apiPost("/settings", {key, value})` |

## Prices

| Old invoke | New api call |
|---|---|
| `invoke("resolve_instruments")` | `apiPost("/prices/resolve-instruments", {})` |
| `invoke("sync_prices", {force})` | `apiPost("/prices/sync", {force})` |

## Import

| Old invoke | New api call |
|---|---|
| `invoke("get_import_sources")` | `apiGet("/import/sources")` |
| `invoke("parse_*_pdf", {file_path})` | `apiPost("/import/parse", {source: "SOURCE_ID", file_path, password?})` |
| `invoke("parse_angel_one_xlsx", {file_path})` | `apiPost("/import/parse", {source: "ANGELONE", file_path})` |
| `invoke("import_*", {account_id, data})` | `apiPost("/import/confirm", {source: "SOURCE_ID", account_id, data, file_name?})` |

## Backup

| Old invoke | New api call | Notes |
|---|---|---|
| `invoke("export_data", {pin, password, dest_path})` | `apiPost("/backup/export", {pin, password, dest_path})` | `dest_path` from `native.pickSavePath()` |
| `invoke("import_data", {password, src_path})` | `apiPost("/backup/import", {password, src_path})` | `src_path` from `native.pickFile()` |

## File dialogs (stay as Tauri invoke → `native.*`)

| Old invoke | New call |
|---|---|
| `open()` from plugin-dialog | `native.pickFile(title?)` |
| `save()` from plugin-dialog | `native.pickSavePath(defaultName?)` |
| `readTextFile(path)` from plugin-fs | `native.readTextFile(path)` |
| `writeTextFile(path, content)` from plugin-fs | `native.writeTextFile(path, content)` |
| `openPath(path)` from plugin-opener | `native.openPath(path)` |
