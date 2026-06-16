mod auth;
mod commands;
mod db;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    auth::state::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_dir)?;
            // DB is NOT opened here — it opens only after the user unlocks with their PIN.

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Auth
            commands::auth::is_setup,
            commands::auth::is_unlocked,
            commands::auth::setup,
            commands::auth::login,
            commands::auth::lock,
            commands::auth::recover,
            commands::auth::change_pin,
            // Persons
            commands::person::get_persons,
            commands::person::create_person,
            commands::person::update_person,
            // Portfolios
            commands::portfolio::get_portfolios,
            commands::portfolio::create_portfolio,
            commands::portfolio::rename_portfolio,
            commands::portfolio::delete_portfolio,
            // Accounts
            commands::account::get_accounts,
            commands::account::create_account,
            commands::account::rename_account,
            commands::account::delete_account,
            // Holdings
            commands::holdings::get_holdings,
            commands::holdings::get_portfolio_summary,
            // Instruments
            commands::instrument::search_instruments,
            commands::instrument::get_instrument_types,
            commands::instrument::create_instrument,
            commands::instrument::update_pending_instrument,
            // Transactions
            commands::transaction::get_transactions,
            commands::transaction::get_transactions_count,
            commands::transaction::get_flagged_count,
            commands::transaction::get_import_batch,
            commands::transaction::create_transaction,
            commands::transaction::update_transaction,
            commands::transaction::delete_transaction,
            commands::transaction::dismiss_transaction_flag,
            commands::transaction::re_evaluate_flags,
            commands::transaction::transfer_holding,
            commands::transaction::create_split,
            // Settings
            commands::settings::get_setting,
            commands::settings::set_setting,
            // Prices
            commands::prices::resolve_instruments,
            commands::prices::sync_prices,
            // Reports
            commands::reports::get_capital_gains,
            commands::reports::get_income,
            commands::reports::export_tax_report,
            // Charges
            commands::charges::get_charges,
            commands::charges::create_charge,
            commands::charges::update_charge,
            commands::charges::delete_charge,
            // Tax ledger
            commands::tax::get_tax_entries,
            commands::tax::create_tax_entry,
            commands::tax::delete_tax_entry,
            // Import parsers
            commands::import::get_import_sources,
            commands::import::cams_cas::parse_cams_cas_pdf,
            commands::import::cams_cas::import_cams_cas,
            commands::import::angel_one::parse_angel_one_xlsx,
            commands::import::angel_one::import_angel_one_trades,
            commands::import::choice_mf::parse_choice_mf_pdf,
            commands::import::choice_mf::import_choice_mf_transactions,
            commands::import::ce_global::parse_ce_global_pdf,
            commands::import::ce_global::import_ce_global_trades,
            commands::import::icici_equity_2::parse_icici_equity_pdf,
            commands::import::icici_equity_2::import_icici_equity_trades,
            commands::import::cn_choice_equity::parse_cn_choice_equity_pdf,
            commands::import::cn_choice_equity::import_cn_choice_equity_trades,
            commands::import::cn_woodstock::parse_cn_woodstock_pdf,
            commands::import::cn_woodstock::import_cn_woodstock_trades,
            commands::import::cn_nirmal_bang::parse_cn_nirmal_bang_pdf,
            commands::import::cn_nirmal_bang::import_cn_nirmal_bang_trades,
            // Backup / Sync
            commands::backup::pick_backup_folder,
            commands::backup::export_data,
            commands::backup::import_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
