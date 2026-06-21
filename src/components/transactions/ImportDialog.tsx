import { useState, useEffect } from "react";
import { apiGet, apiPost, apiPatch } from "@/lib/api";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription,
} from "@/components/ui/dialog";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { formatINR } from "@/lib/format";
import {
  PersonPortfolioAccountSelector,
  type Person, type Portfolio, type Account, type Selection,
} from "@/components/shared/PersonPortfolioAccountSelector";

// ─── Types ────────────────────────────────────────────────────────────────────

interface ImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onImported: () => void;
}

type Step = "selector" | "pick" | "password" | "fix" | "preview" | "importing" | "done";
type ImportSource = "ANGELONE" | "CHOICE_MF" | "CE_GLOBAL" | "ICICI_EQUITY" | "CAMS_CAS" | "CN_CHOICE_EQUITY" | "CN_WOODSTOCK" | "CN_NIRMAL_BANG" | "BAJAJ_FINANCE" | "INVEST_PLUS_OPENING_STOCK";

interface AngelOneParsed {
  trades: AngelOneTrade[];
  unmatched_scrips: string[];
  date_range: [string, string];
  client_code: string;
}
interface AngelOneTrade {
  trade_date: string; scrip_name: string; side: string;
  price_rs: number; quantity: number; order_type: string;
  trade_id: string; total_charges_rs: number;
}

interface ChoiceMfParsed {
  transactions: ChoiceMfTxn[];
  total_rows: number; skipped_rows: number;
}
interface ChoiceMfTxn {
  trade_date: string; scheme_name: string; txn_type: string;
  units: number; nav_rs: number; amount_rs: number; isin: string;
}

interface SkippedRow {
  security_name: string; raw_date: string;
  buy_qty: string; buy_price: string;
  sell_qty: string; sell_price: string; reason: string;
}

interface ChoiceEquityParsed {
  transactions: ChoiceEquityTrade[];
  total_rows: number; skipped_rows: number; intraday_rows: number;
  client_id?: string; client_name?: string;
  skipped_details: SkippedRow[];
  charges: { charge_type: string; amount_paise: number }[];
  statement_start?: string; statement_end?: string;
  pages_scanned: number; pages_with_grid: number;
}
interface ChoiceEquityTrade {
  trade_date: string; security_name: string; exchange_code: string;
  txn_type: string; quantity: number; price: number;
}

interface IciciEquityParsed {
  transactions: IciciEquityTrade[];
  contract_charges: IciciContractNoteCharges[];
  total_rows: number; skipped_rows: number;
  client_code: string; date_range: [string, string];
}
interface IciciEquityTrade {
  trade_date: string; trade_time?: string; cn_no: string;
  security_name: string; isin: string; exchange: string;
  txn_type: string; quantity: number; price: number; brokerage: number; broker_ref?: string;
}
interface IciciContractNoteCharges {
  cn_no: string; trade_date: string;
  stt_paise: number; stamp_charges_paise: number; gst_paise: number;
  trans_charges_paise: number; other_charges_paise: number; total_payable_paise: number;
}

interface ParsedCharge { charge_type: string; amount_paise: number; }

interface CnEquityRow {
  isin: string; security_name: string; bse_code: string;
  txn_type: string; quantity: number; price: number;
}
interface CnChoiceEquityFileParsed {
  trade_date: string; cn_number: string;
  client_code?: string; client_name?: string;
  equity_rows: CnEquityRow[];
  deriv_rows: { contract_desc: string; txn_type: string; quantity: number; price: number }[];
  charges: ParsedCharge[];
  pages_scanned: number; skipped_rows: number;
  _file_path: string;
}

interface WoodstockTrade { isin: string; scrip_name: string; buy_sell: string; quantity: number; price: number; }
interface WoodstockCnFileParsed {
  trade_date: string; cn_number: string; client_code?: string;
  trades: WoodstockTrade[];
  charges: ParsedCharge[];
  pages_scanned: number;
  _file_path: string;
}

interface NirmalBangTrade { isin: string; security_name: string; buy_sell: string; quantity: number; price: number; }
interface NirmalBangCnFileParsed {
  trade_date: string; cn_number: string; client_code?: string;
  trades: NirmalBangTrade[];
  charges: ParsedCharge[];
  pages_scanned: number;
  _file_path: string;
}

interface BajajTrade {
  isin: string; name: string; side: string;
  quantity: number; price_rs: number; trade_date: string;
  contract_note_no?: string;
}
interface BajajParsed {
  trades: BajajTrade[];
  trade_date: string; contract_note_no: string; client_code: string;
  stt_paise: number; gst_paise: number; exchange_paise: number;
  stamp_paise: number; other_paise: number; total_payable_paise: number;
}

interface InvestPlusLot {
  broker: string; name: string; quantity: number;
  price_rs: number; amount_rs: number; trade_date: string;
}
interface InvestPlusParsed {
  lots: InvestPlusLot[];
  portfolio_name: string; financial_year: string;
  opening_date: string; total_lots: number;
}

type AnyParsed = AngelOneParsed | ChoiceMfParsed | ChoiceEquityParsed | IciciEquityParsed
               | CnChoiceEquityFileParsed[] | WoodstockCnFileParsed[] | NirmalBangCnFileParsed[]
               | BajajParsed | InvestPlusParsed;

interface CasTransaction {
  date: string; description: string; txn_type: string;
  amount_rs: number; units: number; nav_rs: number; unit_balance: number;
  stamp_duty_rs: number; stt_rs: number;
}
interface CasFundPreview {
  amc: string; scheme: string; isin: string; folio: string; pan: string;
  opening_balance: number; closing_balance: number; transactions: CasTransaction[];
}
interface CasFundAssignment {
  portfolio_id: number;
  fund: CasFundPreview;
}
interface CasPreview {
  investor_name: string; pan: string; period_from: string; period_to: string;
  funds: CasFundPreview[]; total_transactions: number;
}
interface CasImportFundResult {
  isin: string; scheme: string; folio: string; pan: string;
  transactions_imported: number; transactions_skipped: number;
}
interface CasImportResult {
  funds_imported: number; transactions_imported: number; transactions_skipped: number;
  instruments_auto_created: number; fund_results: CasImportFundResult[];
}

interface ImportSourceMeta { value: ImportSource; label: string; description: string; }

// ─── Component ────────────────────────────────────────────────────────────────

export function ImportDialog({ open: isOpen, onOpenChange, onImported }: ImportDialogProps) {
  // Step flow:  selector → pick → [password] → [fix] → preview → importing → done
  // CAMS CAS:  pick → preview → importing → done  (no selector, no password step for now)

  const [step, setStep]           = useState<Step>("selector");
  const [source, setSource]       = useState<ImportSource | "">("");
  const [filePaths, setFilePaths] = useState<string[]>([]);
  const [parsing, setParsing]     = useState(false);
  const [parseError, setParseError] = useState("");

  // Person → Portfolio → Account selection
  const [persons,     setPersons]     = useState<Person[]>([]);
  const [portfolios,  setPortfolios]  = useState<Portfolio[]>([]);
  const [allAccounts, setAllAccounts] = useState<Account[]>([]);
  const [selection,   setSelection]   = useState<Partial<Selection>>({});
  const [selectorError, setSelectorError] = useState("");

  // Password step
  const [passwordInput, setPasswordInput]     = useState("");
  const [saveForSource, setSaveForSource]     = useState(false);
  const [saveAsPan,     setSaveAsPan]         = useState(false);
  const [passwordError, setPasswordError]     = useState("");
  const [savingPassword, setSavingPassword]   = useState(false);

  // Parsed result
  const [parsed, setParsed] = useState<AnyParsed | null>(null);

  // Sources list
  const [sources, setSources] = useState<ImportSourceMeta[]>([]);

  // Fix-issues step
  interface FixableRow extends SkippedRow {
    ignore: boolean; fixed_date: string; fixed_txn_type: string;
    fixed_qty: string; fixed_price: string;
  }
  const [fixableRows, setFixableRows] = useState<FixableRow[]>([]);

  // CAMS CAS state
  const [casPreview,          setCasPreview]          = useState<CasPreview | null>(null);
  const [casPortfolioByPan,   setCasPortfolioByPan]   = useState<Record<string, string>>({});
  const [casCreatingForPan,   setCasCreatingForPan]   = useState<string | null>(null);
  const [casNewPortfolioName, setCasNewPortfolioName] = useState("");
  const [casPortfolioError,   setCasPortfolioError]   = useState("");
  const [casImportResult,     setCasImportResult]     = useState<CasImportResult | null>(null);

  // Import result
  interface SkippedDetail { trade_date: string; security_name: string; txn_type: string; quantity: number; price: number; reason: string; }
  const [importResult, setImportResult] = useState<{ imported: number; skipped: number; unmatched?: string[]; autoCreated?: string[]; skippedDetails?: SkippedDetail[] } | null>(null);
  const [importError,  setImportError]  = useState("");

  // ── Load data when dialog opens ───────────────────────────────────────────
  useEffect(() => {
    if (isOpen) {
      apiGet<Person[]>("/persons").then(setPersons).catch(() => {});
      apiGet<Portfolio[]>("/portfolios").then(setPortfolios).catch(() => {});
      apiGet<Account[]>("/accounts").then(setAllAccounts).catch(() => {});
      apiGet<ImportSourceMeta[]>("/import/sources").then(setSources).catch(() => {});
    }
  }, [isOpen]);

  // ── Reset ─────────────────────────────────────────────────────────────────
  const reset = () => {
    setStep("selector"); setSource(""); setFilePaths([]);
    setParsing(false); setParseError(""); setParsed(null);
    setSelection({}); setSelectorError("");
    setPasswordInput(""); setSaveForSource(false); setSaveAsPan(false);
    setPasswordError(""); setSavingPassword(false);
    setCasPreview(null); setCasPortfolioByPan({}); setCasCreatingForPan(null);
    setCasNewPortfolioName(""); setCasPortfolioError(""); setCasImportResult(null);
    setImportResult(null); setImportError(""); setFixableRows([]);
  };

  // ── Step selector → pick ──────────────────────────────────────────────────
  const handleSelectorContinue = () => {
    if (!source) { setSelectorError("Select a source"); return; }
    if (source !== "CAMS_CAS") {
      if (!selection.account) { setSelectorError("Select a person, portfolio and account"); return; }
    }
    setSelectorError("");
    setStep("pick");
  };

  // ── Step pick: pick files ─────────────────────────────────────────────────
  const handlePickFile = async () => {
    if (source !== "CAMS_CAS" && !selection.account) return;
    const selected = await open({ title: "Select Statement File(s)", multiple: true });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setFilePaths(prev => {
      const existing = new Set(prev);
      return [...prev, ...paths.filter(p => !existing.has(p))];
    });
  };

  const removeFile = (path: string) => setFilePaths(prev => prev.filter(p => p !== path));

  // ── Step pick: parse (with optional password) ─────────────────────────────
  const handleParse = async (pwd: string | null = null) => {
    if (!source || filePaths.length === 0) return;
    setParsing(true);
    setParseError("");
    try {
      const parseBySource = async <T,>(filePath: string) => apiPost<T>("/import/parse", {
        source,
        file_path: filePath,
        account_id: selection.account?.account_id ?? null,
        password: pwd,
      });

      // ── CAMS CAS ─────────────────────────────────────────────────────────
      if (source === "CAMS_CAS") {
        const result = await parseBySource<CasPreview>(filePaths[0]);
        setCasPreview(result);
        if (portfolios.length === 1) {
          const pid = portfolios[0].portfolio_id.toString();
          const byPan: Record<string, string> = {};
          [...new Set(result.funds.map(f => f.pan || "__unknown__"))].forEach(pan => { byPan[pan] = pid; });
          setCasPortfolioByPan(byPan);
        }
        setStep("preview");
        return;
      }

      // ── Non-CAMS sources ──────────────────────────────────────────────────
      let result: AnyParsed;
      if (source === "BAJAJ_FINANCE") {
        const results = await Promise.all(
          filePaths.map(fp => parseBySource<BajajParsed>(fp))
        );
        result = {
          trades: results.flatMap(r => r.trades.map(t => ({ ...t, contract_note_no: r.contract_note_no }))),
          trade_date: results[0]?.trade_date ?? "",
          contract_note_no: results.length === 1 ? results[0].contract_note_no : "",
          client_code: results[0]?.client_code ?? "",
          stt_paise: results.reduce((sum, r) => sum + r.stt_paise, 0),
          stamp_paise: results.reduce((sum, r) => sum + r.stamp_paise, 0),
          gst_paise: results.reduce((sum, r) => sum + r.gst_paise, 0),
          exchange_paise: results.reduce((sum, r) => sum + r.exchange_paise, 0),
          other_paise: results.reduce((sum, r) => sum + r.other_paise, 0),
          total_payable_paise: results.reduce((sum, r) => sum + r.total_payable_paise, 0),
        };
      } else if (source === "ANGELONE") {
        const results = await Promise.all(
          filePaths.map(fp => parseBySource<AngelOneParsed>(fp))
        );
        const mergedA = results.slice(1).reduce((acc, r) => ({
          trades: [...acc.trades, ...r.trades],
          unmatched_scrips: [...new Set([...acc.unmatched_scrips, ...r.unmatched_scrips])],
          date_range: [
            acc.date_range[0] < r.date_range[0] ? acc.date_range[0] : r.date_range[0],
            acc.date_range[1] > r.date_range[1] ? acc.date_range[1] : r.date_range[1],
          ] as [string, string],
          client_code: acc.client_code || r.client_code,
        }), results[0]);
        const seenA = new Set<string>();
        mergedA.trades = mergedA.trades.filter(t => {
          if (!t.trade_id) return true;
          if (seenA.has(t.trade_id)) return false;
          seenA.add(t.trade_id);
          return true;
        });
        result = mergedA;
      } else if (source === "CHOICE_MF") {
        const results = await Promise.all(
          filePaths.map(fp => parseBySource<ChoiceMfParsed>(fp))
        );
        result = results.slice(1).reduce((acc, r) => ({
          transactions: [...acc.transactions, ...r.transactions],
          total_rows:   acc.total_rows + r.total_rows,
          skipped_rows: acc.skipped_rows + r.skipped_rows,
        }), results[0]);
      } else if (source === "ICICI_EQUITY") {
        const results = await Promise.all(
          filePaths.map(fp => parseBySource<IciciEquityParsed>(fp))
        );
        const merged = results.slice(1).reduce((acc, r) => ({
          transactions:     [...acc.transactions, ...r.transactions],
          contract_charges: [...acc.contract_charges, ...r.contract_charges],
          total_rows:       acc.total_rows + r.total_rows,
          skipped_rows:     acc.skipped_rows + r.skipped_rows,
          client_code:      acc.client_code || r.client_code,
          date_range: [
            acc.date_range[0] < r.date_range[0] ? acc.date_range[0] : r.date_range[0],
            acc.date_range[1] > r.date_range[1] ? acc.date_range[1] : r.date_range[1],
          ] as [string, string],
        }), results[0]);
        const seenCn = new Set<string>();
        merged.transactions = merged.transactions.filter(t => {
          if (seenCn.has(t.cn_no)) return false;
          return true;
        });
        const seenCnCharges = new Set<string>();
        merged.contract_charges = merged.contract_charges.filter(c => {
          if (seenCnCharges.has(c.cn_no)) return false;
          seenCnCharges.add(c.cn_no);
          return true;
        });
        const seenRef = new Set<string>();
        merged.transactions = merged.transactions.filter(t => {
          if (!t.broker_ref) return true;
          if (seenRef.has(t.broker_ref)) return false;
          seenRef.add(t.broker_ref);
          return true;
        });
        merged.total_rows = merged.transactions.length;
        result = merged;
      } else if (source === "CN_CHOICE_EQUITY") {
        const files = await Promise.all(
          filePaths.map(async fp => {
            const r = await parseBySource<Omit<CnChoiceEquityFileParsed, "_file_path">>(fp);
            return { ...r, _file_path: fp };
          })
        );
        setParsed(files);
        setStep("preview");
        return;
      } else if (source === "CN_WOODSTOCK") {
        const files = await Promise.all(
          filePaths.map(async fp => {
            const r = await parseBySource<Omit<WoodstockCnFileParsed, "_file_path">>(fp);
            return { ...r, _file_path: fp };
          })
        );
        setParsed(files);
        setStep("preview");
        return;
      } else if (source === "CN_NIRMAL_BANG") {
        const files = await Promise.all(
          filePaths.map(fp => parseBySource<NirmalBangCnFileParsed>(fp))
        );
        setParsed(files);
        setStep("preview");
        return;
      } else if (source === "INVEST_PLUS_OPENING_STOCK") {
        result = await parseBySource<InvestPlusParsed>(filePaths[0]);
      } else {
        // CE_GLOBAL
        const results = await Promise.all(
          filePaths.map(fp => parseBySource<ChoiceEquityParsed>(fp))
        );
        result = results.slice(1).reduce((acc, r) => ({
          transactions:    [...acc.transactions, ...r.transactions],
          total_rows:      acc.total_rows + r.total_rows,
          skipped_rows:    acc.skipped_rows + r.skipped_rows,
          intraday_rows:   (acc.intraday_rows ?? 0) + (r.intraday_rows ?? 0),
          client_id:       acc.client_id || r.client_id,
          client_name:     acc.client_name || r.client_name,
          skipped_details: [...(acc.skipped_details ?? []), ...(r.skipped_details ?? [])],
          charges:         [...(acc.charges ?? []), ...(r.charges ?? [])],
          statement_start: acc.statement_start || r.statement_start,
          statement_end:   r.statement_end || acc.statement_end,
          pages_scanned:   (acc.pages_scanned ?? 0) + (r.pages_scanned ?? 0),
          pages_with_grid: (acc.pages_with_grid ?? 0) + (r.pages_with_grid ?? 0),
        }), results[0]);
      }
      setParsed(result);

      if (source === "CE_GLOBAL") {
        const eq = result as ChoiceEquityParsed;
        if (eq.skipped_details?.length > 0) {
          setFixableRows(eq.skipped_details.map(r => ({
            ...r, ignore: false,
            fixed_date: r.raw_date,
            fixed_txn_type: r.sell_qty && r.sell_qty !== "-" && r.sell_qty !== "0" ? "SELL" : "BUY",
            fixed_qty: r.sell_qty && r.sell_qty !== "-" && r.sell_qty !== "0" ? r.sell_qty : r.buy_qty,
            fixed_price: r.sell_price && r.sell_price !== "-" && r.sell_price !== "0.00" ? r.sell_price : r.buy_price,
          })));
          setStep("fix");
          return;
        }
      }
      setStep("preview");
    } catch (e: any) {
      const msg = typeof e === "string" ? e : e?.message ?? "Parse failed";
      const isPasswordError = msg === "PASSWORD_REQUIRED" || msg.includes("PASSWORD_REQUIRED")
        || msg === "WRONG_PASSWORD" || msg.includes("WRONG_PASSWORD");
      if (isPasswordError) {
        setPasswordInput("");
        setSaveForSource(false);
        setSaveAsPan(false);
        setPasswordError("");
        setStep("password");
      } else {
        setParseError(msg);
      }
    } finally {
      setParsing(false);
    }
  };

  // ── Password step: save settings and retry ────────────────────────────────
  const handlePasswordConfirm = async () => {
    if (!passwordInput.trim()) { setPasswordError("Enter the password"); return; }
    setSavingPassword(true);
    setPasswordError("");
    try {
      if (saveForSource && source && selection.account) {
        await apiPost("/import/password", {
          source,
          account_id: selection.account.account_id,
          password: passwordInput,
        });
      }
      if (saveAsPan && selection.person) {
        await apiPatch(`/persons/${selection.person.person_id}`, { name: null, pan: passwordInput });
        setPersons(ps => ps.map(p => p.person_id === selection.person!.person_id ? { ...p, pan: passwordInput } : p));
      }
    } catch {
      // Non-fatal — still try to parse even if saving failed
    } finally {
      setSavingPassword(false);
    }
    setStep("pick");
    await handleParse(passwordInput);
  };

  // ── Fix step: apply corrections ───────────────────────────────────────────
  const handleApplyFixes = () => {
    if (!parsed || source !== "CE_GLOBAL") { setStep("preview"); return; }
    const eq = parsed as ChoiceEquityParsed;
    const added: ChoiceEquityTrade[] = fixableRows
      .filter(r => !r.ignore)
      .map(r => ({
        trade_date: r.fixed_date, security_name: r.security_name,
        exchange_code: "", txn_type: r.fixed_txn_type,
        quantity: parseFloat(r.fixed_qty.replace(/,/g, "")) || 0,
        price: parseFloat(r.fixed_price.replace(/,/g, "")) || 0,
        trade_segment: "EQ",
      }));
    setParsed({ ...eq, transactions: [...eq.transactions, ...added] });
    setStep("preview");
  };

  // ── CAMS: create portfolio for a PAN group ────────────────────────────────
  const handleCasCreatePortfolio = async () => {
    const name = casNewPortfolioName.trim();
    if (!name || !casCreatingForPan) return;
    try {
      const created = await apiPost<Portfolio>("/portfolios", { name, person_id: null });
      setPortfolios(ps => [...ps, created]);
      setCasPortfolioByPan(prev => ({ ...prev, [casCreatingForPan]: created.portfolio_id.toString() }));
      setCasCreatingForPan(null);
      setCasNewPortfolioName("");
    } catch (e: any) {
      setCasPortfolioError(typeof e === "string" ? e : e?.message ?? "Failed");
    }
  };

  // ── Import ────────────────────────────────────────────────────────────────
  const handleImport = async () => {
    if (source === "CAMS_CAS") {
      if (!casPreview) return;
      const unassigned = casPreview.funds.some(f => !casPortfolioByPan[f.pan || "__unknown__"]);
      if (unassigned) { setCasPortfolioError("Assign a portfolio to every investor"); return; }
      setCasPortfolioError("");
      setStep("importing");
      setImportError("");
      try {
        const assignments: CasFundAssignment[] = casPreview.funds.map(fund => ({
          portfolio_id: parseInt(casPortfolioByPan[fund.pan || "__unknown__"]),
          fund,
        }));
        const result = await apiPost<CasImportResult>("/import/confirm", {
          source,
          account_id: 0,
          data: { assignments },
          file_name: null,
        });
        setCasImportResult(result);
        setStep("done");
      } catch (e: any) {
        setImportError(typeof e === "string" ? e : e?.message ?? "Import failed");
        setStep("preview");
      }
      return;
    }

    if (!selection.account || !parsed) return;
    const accountId = selection.account.account_id;
    setStep("importing");
    setImportError("");
    try {
      const result = await apiPost<{ imported: number; skipped: number; auto_created_instruments?: string[]; skipped_details?: SkippedDetail[] }>(
        "/import/confirm",
        {
          source,
          account_id: accountId,
          data: parsed,
          file_name: filePaths.length === 1 ? filePaths[0] : null,
        },
      );
      setImportResult({
        imported: result.imported,
        skipped: result.skipped,
        autoCreated: Array.isArray(result.auto_created_instruments) ? result.auto_created_instruments : undefined,
        skippedDetails: result.skipped_details,
      });
      setStep("done");
    } catch (e: any) {
      setImportError(typeof e === "string" ? e : e?.message ?? "Import failed");
      setStep("preview");
    }
  };

  // ── Preview rows helper ───────────────────────────────────────────────────
  const previewRows = () => {
    if (!parsed) return null;
    if (source === "ANGELONE") {
      const trades = (parsed as AngelOneParsed).trades.slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th><th className="py-1 text-left">Scrip</th>
            <th className="py-1 text-left">Side</th><th className="py-1 text-right">Price</th>
            <th className="py-1 text-right">Qty</th>
          </tr></thead>
          <tbody>{trades.map((t, i) => (
            <tr key={i} className="border-b border-border/40">
              <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
              <td className="py-1 pr-2 font-medium">{t.scrip_name}</td>
              <td className={`py-1 pr-2 font-medium ${t.side === "BUY" ? "text-green-600 dark:text-green-400" : "text-red-500"}`}>{t.side}</td>
              <td className="py-1 pr-2 text-right">{formatINR(t.price_rs * 100)}</td>
              <td className="py-1 text-right">{t.quantity}</td>
            </tr>
          ))}</tbody>
        </table>
      );
    }
    if (source === "CHOICE_MF") {
      const txns = (parsed as ChoiceMfParsed).transactions.slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th><th className="py-1 text-left">Scheme</th>
            <th className="py-1 text-left">Type</th><th className="py-1 text-right">Units</th>
            <th className="py-1 text-right">Amount</th>
          </tr></thead>
          <tbody>{txns.map((t, i) => (
            <tr key={i} className="border-b border-border/40">
              <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
              <td className="py-1 pr-2 font-medium max-w-[160px] truncate">{t.scheme_name}</td>
              <td className="py-1 pr-2">{t.txn_type}</td>
              <td className="py-1 pr-2 text-right">{t.units.toFixed(3)}</td>
              <td className="py-1 text-right">{formatINR(t.amount_rs * 100)}</td>
            </tr>
          ))}</tbody>
        </table>
      );
    }
    if (source === "CN_CHOICE_EQUITY") {
      const rows = (parsed as CnChoiceEquityFileParsed[])
        .flatMap(f => f.equity_rows.map(r => ({ ...r, trade_date: f.trade_date }))).slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th><th className="py-1 text-left">Security</th>
            <th className="py-1 text-left">Type</th><th className="py-1 text-right">Qty</th>
            <th className="py-1 text-right">Price</th>
          </tr></thead>
          <tbody>{rows.map((t, i) => {
            const txnType = t.txn_type.split("|")[0];
            return (
              <tr key={i} className="border-b border-border/40">
                <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
                <td className="py-1 pr-2 font-medium max-w-[160px] truncate">{t.security_name}</td>
                <td className={`py-1 pr-2 font-medium ${txnType === "BUY" ? "text-green-600 dark:text-green-400" : "text-red-500"}`}>{txnType}</td>
                <td className="py-1 pr-2 text-right">{t.quantity}</td>
                <td className="py-1 text-right">{formatINR(t.price * 100)}</td>
              </tr>
            );
          })}</tbody>
        </table>
      );
    }
    if (source === "CN_WOODSTOCK") {
      const rows = (parsed as WoodstockCnFileParsed[])
        .flatMap(f => f.trades.map(t => ({ ...t, trade_date: f.trade_date }))).slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th><th className="py-1 text-left">Security</th>
            <th className="py-1 text-left">Type</th><th className="py-1 text-right">Qty</th>
            <th className="py-1 text-right">Price</th>
          </tr></thead>
          <tbody>{rows.map((t, i) => (
            <tr key={i} className="border-b border-border/40">
              <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
              <td className="py-1 pr-2 font-medium max-w-[160px] truncate">{t.scrip_name}</td>
              <td className={`py-1 pr-2 font-medium ${t.buy_sell === "BUY" ? "text-green-600 dark:text-green-400" : "text-red-500"}`}>{t.buy_sell}</td>
              <td className="py-1 pr-2 text-right">{t.quantity}</td>
              <td className="py-1 text-right">{formatINR(t.price * 100)}</td>
            </tr>
          ))}</tbody>
        </table>
      );
    }
    if (source === "CN_NIRMAL_BANG") {
      const rows = (parsed as NirmalBangCnFileParsed[])
        .flatMap(f => f.trades.map(t => ({ ...t, trade_date: f.trade_date }))).slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th><th className="py-1 text-left">Security</th>
            <th className="py-1 text-left">Type</th><th className="py-1 text-right">Qty</th>
            <th className="py-1 text-right">Price</th>
          </tr></thead>
          <tbody>{rows.map((t, i) => (
            <tr key={i} className="border-b border-border/40">
              <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
              <td className="py-1 pr-2 font-medium max-w-[160px] truncate">{t.security_name}</td>
              <td className={`py-1 pr-2 font-medium ${t.buy_sell === "BUY" ? "text-green-600 dark:text-green-400" : "text-red-500"}`}>{t.buy_sell}</td>
              <td className="py-1 pr-2 text-right">{t.quantity}</td>
              <td className="py-1 text-right">{formatINR(t.price * 100)}</td>
            </tr>
          ))}</tbody>
        </table>
      );
    }
    if (source === "INVEST_PLUS_OPENING_STOCK") {
      const lots = (parsed as InvestPlusParsed).lots.slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Purchase Date</th><th className="py-1 text-left">Stock</th>
            <th className="py-1 text-right">Qty</th><th className="py-1 text-right">Price</th>
            <th className="py-1 text-right">Amount</th>
          </tr></thead>
          <tbody>{lots.map((t, i) => (
            <tr key={i} className="border-b border-border/40">
              <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
              <td className="py-1 pr-2 font-medium max-w-[160px] truncate">{t.name}</td>
              <td className="py-1 pr-2 text-right">{t.quantity}</td>
              <td className="py-1 pr-2 text-right">{formatINR(t.price_rs * 100)}</td>
              <td className="py-1 text-right">{formatINR(t.amount_rs * 100)}</td>
            </tr>
          ))}</tbody>
        </table>
      );
    }
    if (source === "BAJAJ_FINANCE") {
      const trades = (parsed as BajajParsed).trades.slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th><th className="py-1 text-left">Security</th>
            <th className="py-1 text-left">Side</th><th className="py-1 text-right">Qty</th>
            <th className="py-1 text-right">Price</th>
          </tr></thead>
          <tbody>{trades.map((t, i) => (
            <tr key={i} className="border-b border-border/40">
              <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
              <td className="py-1 pr-2 font-medium max-w-[160px] truncate">{t.name}</td>
              <td className={`py-1 pr-2 font-medium ${t.side === "BUY" ? "text-green-600 dark:text-green-400" : "text-red-500"}`}>{t.side}</td>
              <td className="py-1 pr-2 text-right">{t.quantity}</td>
              <td className="py-1 text-right">{formatINR(t.price_rs * 100)}</td>
            </tr>
          ))}</tbody>
        </table>
      );
    }
    const trades = source === "ICICI_EQUITY"
      ? (parsed as IciciEquityParsed).transactions.slice(0, 10)
      : (parsed as ChoiceEquityParsed).transactions.slice(0, 10);
    return (
      <table className="w-full text-xs">
        <thead><tr className="border-b text-muted-foreground">
          <th className="py-1 text-left">Date</th><th className="py-1 text-left">Security</th>
          <th className="py-1 text-left">Side</th><th className="py-1 text-right">Qty</th>
          <th className="py-1 text-right">Price</th>
        </tr></thead>
        <tbody>{trades.map((t, i) => (
          <tr key={i} className="border-b border-border/40">
            <td className="py-1 pr-2 text-muted-foreground">{t.trade_date}</td>
            <td className="py-1 pr-2 font-medium max-w-[160px] truncate">{t.security_name}</td>
            <td className={`py-1 pr-2 font-medium ${t.txn_type === "BUY" ? "text-green-600 dark:text-green-400" : "text-red-500"}`}>{t.txn_type}</td>
            <td className="py-1 pr-2 text-right">{t.quantity}</td>
            <td className="py-1 text-right">{formatINR(t.price * 100)}</td>
          </tr>
        ))}</tbody>
      </table>
    );
  };

  const totalRows = parsed
    ? source === "ANGELONE"                   ? (parsed as AngelOneParsed).trades.length
    : source === "BAJAJ_FINANCE"              ? (parsed as BajajParsed).trades.length
    : source === "INVEST_PLUS_OPENING_STOCK" ? (parsed as InvestPlusParsed).total_lots
    : source === "CE_GLOBAL"        ? (parsed as ChoiceEquityParsed).total_rows
    : source === "ICICI_EQUITY"     ? (parsed as IciciEquityParsed).total_rows
    : source === "CN_CHOICE_EQUITY" ? (parsed as CnChoiceEquityFileParsed[]).reduce((s, f) => s + f.equity_rows.length + f.deriv_rows.length, 0)
    : source === "CN_WOODSTOCK"     ? (parsed as WoodstockCnFileParsed[]).reduce((s, f) => s + f.trades.length, 0)
    : source === "CN_NIRMAL_BANG"   ? (parsed as NirmalBangCnFileParsed[]).reduce((s, f) => s + f.trades.length, 0)
    : (parsed as ChoiceMfParsed).total_rows
    : 0;

  const sourceLabel = sources.find(s => s.value === source)?.label ?? source;
  const selectedPersonName = selection.person?.name ?? "";
  const selectedPersonHasPan = !!selection.person?.pan;

  // ── Render ────────────────────────────────────────────────────────────────
  return (
    <Dialog open={isOpen} onOpenChange={(o) => { if (!o) reset(); onOpenChange(o); }}>
      <DialogContent className="w-[900px] sm:max-w-[900px]">
        <DialogHeader>
          <DialogTitle>
            {step === "selector"  && "Import Statement"}
            {step === "pick"      && "Upload Files"}
            {step === "password"  && "Password Required"}
            {step === "fix"       && "Fix Parsing Issues"}
            {step === "preview"   && "Preview & Import"}
            {step === "importing" && "Importing…"}
            {step === "done"      && "Import Complete"}
          </DialogTitle>
          <DialogDescription>
            {step === "selector"  && "Choose the statement source and the account to import into."}
            {step === "pick"      && `Upload your ${sourceLabel} statement file${source !== "ANGELONE" ? " (PDF)" : " (XLSX)"}.`}
            {step === "password"  && "This PDF is password-protected. Enter the password to continue."}
            {step === "fix"       && "Some rows could not be parsed. Correct them before continuing."}
            {step === "preview"   && (source === "CAMS_CAS" && casPreview
              ? `${casPreview.funds.length} fund${casPreview.funds.length !== 1 ? "s" : ""} · ${casPreview.total_transactions} transactions · ${casPreview.period_from} → ${casPreview.period_to}`
              : `${filePaths.length > 1 ? `${filePaths.length} files · ` : ""}${totalRows} transactions found. Review and confirm.`)}
            {step === "done"      && "Transactions have been added."}
          </DialogDescription>
        </DialogHeader>

        {/* ── Step: selector ── */}
        {step === "selector" && (
          <div className="space-y-4 py-1 min-h-[300px]">
            <div className="space-y-1.5">
              <Label>Statement source</Label>
              <Select value={source} onValueChange={(v) => { setSource(v as ImportSource); setSelectorError(""); }}>
                <SelectTrigger className="w-2/3"><SelectValue placeholder="Select source" /></SelectTrigger>
                <SelectContent>
                  {sources.map(s => (
                    <SelectItem key={s.value} value={s.value}>
                      <div>
                        <div className="font-medium">{s.label}</div>
                        <div className="text-xs text-muted-foreground">{s.description}</div>
                      </div>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {source && source !== "CAMS_CAS" && (
              <div className="border rounded-lg p-3 space-y-1">
                <p className="text-xs text-muted-foreground mb-2 font-medium uppercase tracking-wide">Import into account</p>
                <PersonPortfolioAccountSelector
                  persons={persons}
                  portfolios={portfolios}
                  accounts={allAccounts}
                  value={selection}
                  onChange={setSelection}
                  onPersonCreated={(p) => setPersons(ps => [...ps, p])}
                  onPortfolioCreated={(p) => setPortfolios(ps => [...ps, p])}
                  onAccountCreated={(a) => setAllAccounts(as => [...as, a])}
                  showAccount={true}
                />
              </div>
            )}

            {source === "CAMS_CAS" && (
              <div className="rounded-md bg-muted/40 border px-3 py-2.5 text-sm text-muted-foreground">
                For CAMS CAS files you'll assign portfolios to each investor during the preview step.
              </div>
            )}

            {selectorError && (
              <p className="text-sm text-destructive">{selectorError}</p>
            )}
          </div>
        )}

        {/* ── Step: pick ── */}
        {step === "pick" && (
          <div className="space-y-4 py-1 min-h-[300px]">
            {selection.account && (
              <div className="text-xs text-muted-foreground">
                Importing into: <span className="font-medium text-foreground">{selection.account.name}</span>
                {selection.portfolio && <span> · {selection.portfolio.name}</span>}
              </div>
            )}
            <div className="space-y-1.5">
              <Label>Statement file{filePaths.length !== 1 ? "s" : ""}</Label>
              <div className="h-[170px] overflow-y-auto space-y-1 pr-0.5">
                {filePaths.map(fp => (
                  <div key={fp} className="flex items-center gap-2 border rounded-md px-3 py-1.5 bg-muted/30">
                    <span className="flex-1 text-sm truncate">{fp.split(/[/\\]/).pop()}</span>
                    <button
                      type="button"
                      onClick={() => removeFile(fp)}
                      className="text-muted-foreground hover:text-destructive transition-colors shrink-0"
                      title="Remove"
                    >
                      ✕
                    </button>
                  </div>
                ))}
              </div>
              <Button
                variant="outline"
                onClick={handlePickFile}
                type="button"
                className="w-full"
                disabled={source !== "CAMS_CAS" && !selection.account}
              >
                {filePaths.length > 0 ? "Add more files" : "Browse…"}
              </Button>
              {source !== "CAMS_CAS" && !selection.account && (
                <p className="text-xs text-muted-foreground">Select a person, portfolio and account first.</p>
              )}
            </div>

            {parseError && (
              <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
                {parseError}
              </div>
            )}
          </div>
        )}

        {/* ── Step: password ── */}
        {step === "password" && (
          <div className="space-y-4 py-2">
            <div className="space-y-2">
              <Label>Password</Label>
              <Input
                type="password"
                autoFocus
                placeholder="Enter PDF password"
                value={passwordInput}
                onChange={e => setPasswordInput(e.target.value)}
                onKeyDown={e => e.key === "Enter" && handlePasswordConfirm()}
              />
            </div>
            <div className="space-y-2">
              <label className="flex items-start gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  className="mt-0.5"
                  checked={saveForSource}
                  onChange={e => setSaveForSource(e.target.checked)}
                />
                <div>
                  <div className="text-sm font-medium">Remember this password for {sourceLabel} on this account</div>
                  <div className="text-xs text-muted-foreground">Saved to the import password store and auto-tried next time for the same account and source.</div>
                </div>
              </label>
              {selectedPersonName && (
                <label className="flex items-start gap-2.5 cursor-pointer">
                  <input
                    type="checkbox"
                    className="mt-0.5"
                    checked={saveAsPan}
                    onChange={e => setSaveAsPan(e.target.checked)}
                    disabled={selectedPersonHasPan}
                  />
                  <div>
                    <div className={`text-sm font-medium ${selectedPersonHasPan ? "text-muted-foreground" : ""}`}>
                      Save as PAN for {selectedPersonName}
                      {selectedPersonHasPan && <span className="ml-1 text-xs">(already set)</span>}
                    </div>
                    <div className="text-xs text-muted-foreground">Stores the password as the PAN on this person's profile.</div>
                  </div>
                </label>
              )}
            </div>
            {passwordError && (
              <p className="text-sm text-destructive">{passwordError}</p>
            )}
          </div>
        )}

        {/* ── Step: fix ── */}
        {step === "fix" && (
          <div className="space-y-3 py-1">
            <p className="text-sm text-muted-foreground">
              {fixableRows.length} row{fixableRows.length !== 1 ? "s" : ""} could not be parsed automatically.
              Correct them below or check "Ignore" to skip.
            </p>
            <div className="space-y-3 max-h-80 overflow-y-auto pr-1">
              {fixableRows.map((r, i) => (
                <div key={i} className={`border rounded-md p-3 space-y-2 text-sm ${r.ignore ? "opacity-50" : ""}`}>
                  <div className="flex items-start justify-between gap-2">
                    <div className="font-medium truncate">{r.security_name || "(unknown)"}</div>
                    <label className="flex items-center gap-1.5 text-xs text-muted-foreground shrink-0 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={r.ignore}
                        onChange={e => setFixableRows(rows => rows.map((row, j) =>
                          j === i ? { ...row, ignore: e.target.checked } : row
                        ))}
                      />
                      Ignore
                    </label>
                  </div>
                  <div className="text-xs text-amber-600 dark:text-amber-400">{r.reason}</div>
                  {!r.ignore && (
                    <div className="grid grid-cols-2 gap-2">
                      <div className="space-y-1">
                        <label className="text-xs text-muted-foreground">Date (YYYY-MM-DD)</label>
                        <Input className="h-7 text-xs" value={r.fixed_date} placeholder="YYYY-MM-DD"
                          onChange={e => setFixableRows(rows => rows.map((row, j) => j === i ? { ...row, fixed_date: e.target.value } : row))} />
                      </div>
                      <div className="space-y-1">
                        <label className="text-xs text-muted-foreground">Type</label>
                        <select
                          className="w-full h-7 text-xs rounded-md border border-input bg-background px-2"
                          value={r.fixed_txn_type}
                          onChange={e => setFixableRows(rows => rows.map((row, j) => j === i ? { ...row, fixed_txn_type: e.target.value } : row))}
                        >
                          <option value="BUY">BUY</option>
                          <option value="SELL">SELL</option>
                        </select>
                      </div>
                      <div className="space-y-1">
                        <label className="text-xs text-muted-foreground">Quantity</label>
                        <Input className="h-7 text-xs" value={r.fixed_qty}
                          onChange={e => setFixableRows(rows => rows.map((row, j) => j === i ? { ...row, fixed_qty: e.target.value } : row))} />
                      </div>
                      <div className="space-y-1">
                        <label className="text-xs text-muted-foreground">Price (₹)</label>
                        <Input className="h-7 text-xs" value={r.fixed_price}
                          onChange={e => setFixableRows(rows => rows.map((row, j) => j === i ? { ...row, fixed_price: e.target.value } : row))} />
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {/* ── Step: preview (CAMS CAS) ── */}
        {step === "preview" && source === "CAMS_CAS" && casPreview && (() => {
          const pans = [...new Set(casPreview.funds.map(f => f.pan || "__unknown__"))];
          return (
            <div className="space-y-4 py-1">
              {pans.map(pan => {
                const panFunds = casPreview.funds.filter(f => (f.pan || "__unknown__") === pan);
                const assignedPid = casPortfolioByPan[pan] ?? "";
                return (
                  <div key={pan} className="space-y-2">
                    <div className="flex items-center justify-between gap-3">
                      <div className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                        {pan === "__unknown__" ? "Unknown investor" : `PAN: ${pan}`}
                        <span className="ml-2 normal-case font-normal">
                          · {panFunds.length} fund{panFunds.length !== 1 ? "s" : ""}
                          · {panFunds.reduce((s, f) => s + f.transactions.length, 0)} txns
                        </span>
                      </div>
                      {casCreatingForPan === pan ? (
                        <div className="flex gap-1.5 shrink-0">
                          <Input
                            autoFocus className="h-7 text-xs w-36" placeholder="Portfolio name"
                            value={casNewPortfolioName} onChange={e => setCasNewPortfolioName(e.target.value)}
                            onKeyDown={async e => {
                              if (e.key === "Enter") await handleCasCreatePortfolio();
                              if (e.key === "Escape") { setCasCreatingForPan(null); setCasNewPortfolioName(""); }
                            }}
                          />
                          <Button size="xs" onClick={handleCasCreatePortfolio} disabled={!casNewPortfolioName.trim()}>Add</Button>
                          <Button size="xs" variant="outline" onClick={() => { setCasCreatingForPan(null); setCasNewPortfolioName(""); }}>✕</Button>
                        </div>
                      ) : (
                        <Select value={assignedPid} onValueChange={v => {
                          if (v === "__new__") { setCasCreatingForPan(pan); setCasNewPortfolioName(""); return; }
                          setCasPortfolioByPan(prev => ({ ...prev, [pan]: v ?? "" }));
                        }}>
                          <SelectTrigger className="h-7 text-xs w-40 shrink-0"><SelectValue placeholder="Assign portfolio" /></SelectTrigger>
                          <SelectContent>
                            {portfolios.map(p => (
                              <SelectItem key={p.portfolio_id} value={p.portfolio_id.toString()}>{p.name}</SelectItem>
                            ))}
                            <SelectItem value="__new__" className="text-primary font-medium">+ Create new</SelectItem>
                          </SelectContent>
                        </Select>
                      )}
                    </div>
                    <div className="space-y-1">
                      {panFunds.map((fund, i) => (
                        <div key={i} className="border rounded-md px-3 py-2 bg-muted/20 text-sm flex items-start justify-between gap-2">
                          <div className="min-w-0">
                            <div className="font-medium truncate">{fund.scheme || fund.isin}</div>
                            <div className="text-xs text-muted-foreground mt-0.5 flex gap-2 flex-wrap">
                              <span>{fund.isin}</span>
                              {fund.folio && <span>· Folio {fund.folio}</span>}
                            </div>
                          </div>
                          <div className="text-right shrink-0 text-xs text-muted-foreground">
                            <div className="font-medium text-foreground">{fund.transactions.length} txns</div>
                            {fund.closing_balance > 0 && <div>{fund.closing_balance.toFixed(3)} units</div>}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                );
              })}
              {casPortfolioError && (
                <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">{casPortfolioError}</div>
              )}
              {importError && (
                <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">{importError}</div>
              )}
            </div>
          );
        })()}

        {/* ── Step: preview (equity/MF sources) ── */}
        {step === "preview" && source !== "CAMS_CAS" && parsed && (
          <div className="space-y-3 py-1">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <span>Account: <span className="font-medium text-foreground">{selection.account?.name}</span></span>
              <span>·</span>
              <span>{totalRows} transactions</span>
            </div>
            <div className="border rounded-md overflow-auto max-h-56 px-3 py-2 bg-muted/20">
              {previewRows()}
              {totalRows > 10 && (
                <p className="text-xs text-muted-foreground mt-2">…and {totalRows - 10} more</p>
              )}
            </div>
            {source === "ANGELONE" && (parsed as AngelOneParsed).unmatched_scrips.length > 0 && (
              <div className="text-xs text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-md px-3 py-2">
                {(parsed as AngelOneParsed).unmatched_scrips.length} scrip(s) not in instrument database — placeholder instruments will be auto-created and enriched when prices sync.
              </div>
            )}
            {source === "CE_GLOBAL" && (parsed as ChoiceEquityParsed).pages_with_grid === 0 && (parsed as ChoiceEquityParsed).pages_scanned > 0 && (
              <div className="text-xs text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 rounded-md px-3 py-2">
                No border grid detected across {(parsed as ChoiceEquityParsed).pages_scanned} page(s). This PDF may be a different report format — please upload a <strong>Global Details Report</strong> from Choice Equity.
              </div>
            )}
            {source === "CE_GLOBAL" && (parsed as ChoiceEquityParsed).intraday_rows > 0 && (
              <div className="text-xs text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-md px-3 py-2">
                {(parsed as ChoiceEquityParsed).intraday_rows} intraday round-trip(s) detected — imported as both BUY and SELL legs.
              </div>
            )}
            {importError && (
              <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">{importError}</div>
            )}
          </div>
        )}

        {/* ── Step: importing ── */}
        {step === "importing" && (
          <div className="py-6 text-center text-sm text-muted-foreground">Importing transactions…</div>
        )}

        {/* ── Step: done (CAMS CAS) ── */}
        {step === "done" && source === "CAMS_CAS" && casImportResult && (
          <div className="space-y-3 py-2">
            <div className="text-sm font-medium text-green-700 dark:text-green-400">
              {casImportResult.transactions_imported} transaction{casImportResult.transactions_imported !== 1 ? "s" : ""} imported across {casImportResult.funds_imported} fund{casImportResult.funds_imported !== 1 ? "s" : ""}.
            </div>
            {casImportResult.instruments_auto_created > 0 && (
              <div className="text-xs text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-md px-3 py-2">
                {casImportResult.instruments_auto_created} placeholder instrument{casImportResult.instruments_auto_created !== 1 ? "s" : ""} created.
              </div>
            )}
            {casImportResult.transactions_skipped > 0 && (
              <div className="text-xs text-muted-foreground">{casImportResult.transactions_skipped} duplicate or zero-value transactions skipped.</div>
            )}
            <div className="space-y-1 max-h-48 overflow-y-auto">
              {casImportResult.fund_results.map((f, i) => (
                <div key={i} className="flex items-center justify-between text-xs py-1 border-b border-border/40 last:border-0">
                  <div className="min-w-0">
                    <span className="font-medium truncate block">{f.scheme || f.isin}</span>
                    {f.folio && <span className="text-muted-foreground">Folio {f.folio}</span>}
                  </div>
                  <div className="shrink-0 ml-4 text-right">
                    <span className="text-green-700 dark:text-green-400">{f.transactions_imported} imported</span>
                    {f.transactions_skipped > 0 && <span className="text-muted-foreground ml-2">{f.transactions_skipped} skipped</span>}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* ── Step: done (equity/MF sources) ── */}
        {step === "done" && source !== "CAMS_CAS" && importResult && (
          <div className="space-y-3 py-2">
            <div className="text-sm font-medium text-green-700 dark:text-green-400">
              {importResult.imported} transaction{importResult.imported !== 1 ? "s" : ""} imported successfully.
            </div>
            {importResult.autoCreated && importResult.autoCreated.length > 0 && (
              <div className="text-xs text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-md px-3 py-2">
                {importResult.autoCreated.length} placeholder instrument{importResult.autoCreated.length !== 1 ? "s" : ""} auto-created:{" "}
                {importResult.autoCreated.slice(0, 8).join(", ")}
                {importResult.autoCreated.length > 8 && "…"}
              </div>
            )}
            {importResult.skippedDetails && importResult.skippedDetails.length > 0 && (
              <div className="space-y-1.5">
                <div className="text-xs font-medium text-muted-foreground">{importResult.skippedDetails.length} skipped</div>
                <div className="max-h-48 overflow-y-auto rounded-md border text-xs">
                  <table className="w-full">
                    <thead className="sticky top-0 bg-muted/80">
                      <tr className="border-b text-muted-foreground">
                        <th className="py-1 px-2 text-left">Date</th>
                        <th className="py-1 px-2 text-left">Security</th>
                        <th className="py-1 px-2 text-left">Type</th>
                        <th className="py-1 px-2 text-right">Qty</th>
                        <th className="py-1 px-2 text-left">Reason</th>
                      </tr>
                    </thead>
                    <tbody>
                      {importResult.skippedDetails.map((r, i) => (
                        <tr key={i} className="border-b border-border/40 last:border-0">
                          <td className="py-1 px-2 text-muted-foreground whitespace-nowrap">{r.trade_date}</td>
                          <td className="py-1 px-2 max-w-[140px] truncate">{r.security_name}</td>
                          <td className={`py-1 px-2 ${r.txn_type === "BUY" ? "text-blue-600 dark:text-blue-400" : "text-red-500"}`}>{r.txn_type}</td>
                          <td className="py-1 px-2 text-right">{r.quantity}</td>
                          <td className="py-1 px-2 text-muted-foreground">{r.reason}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </div>
        )}

        {/* ── Footer ── */}
        <DialogFooter>
          {step !== "done" && step !== "importing" && (
            <Button variant="outline" onClick={() => { reset(); onOpenChange(false); }}>Cancel</Button>
          )}

          {step === "selector" && (
            <Button onClick={handleSelectorContinue} disabled={!source}>
              Continue
            </Button>
          )}

          {step === "pick" && (
            <>
              <Button variant="outline" onClick={() => setStep("selector")}>Back</Button>
              <Button onClick={() => handleParse(null)} disabled={filePaths.length === 0 || parsing}>
                {parsing ? "Parsing…" : filePaths.length > 1 ? `Parse ${filePaths.length} Files` : "Parse File"}
              </Button>
            </>
          )}

          {step === "password" && (
            <>
              <Button variant="outline" onClick={() => setStep("pick")}>Back</Button>
              <Button onClick={handlePasswordConfirm} disabled={!passwordInput.trim() || savingPassword || parsing}>
                {parsing ? "Parsing…" : "Try Password"}
              </Button>
            </>
          )}

          {step === "fix" && (
            <>
              <Button variant="outline" onClick={() => setStep("pick")}>Back</Button>
              <Button onClick={handleApplyFixes}>
                Continue with {fixableRows.filter(r => !r.ignore).length} fix{fixableRows.filter(r => !r.ignore).length !== 1 ? "es" : ""}
              </Button>
            </>
          )}

          {step === "preview" && source === "CAMS_CAS" && (
            <>
              <Button variant="outline" onClick={() => setStep("pick")}>Back</Button>
              <Button onClick={handleImport}>Import {casPreview?.total_transactions} transactions</Button>
            </>
          )}

          {step === "preview" && source !== "CAMS_CAS" && (
            <>
              <Button variant="outline" onClick={() => setStep("pick")}>Back</Button>
              <Button onClick={handleImport}>Import {totalRows} transactions</Button>
            </>
          )}

          {step === "done" && (
            <Button onClick={() => { reset(); onOpenChange(false); onImported(); }}>Done</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
