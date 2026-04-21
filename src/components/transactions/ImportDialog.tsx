import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
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

// ─── Types ────────────────────────────────────────────────────────────────────

interface Account {
  account_id: number;
  portfolio_id: number;
  name: string;
  account_type: string;
  broker?: string;
  account_no?: string;
}

interface Portfolio { portfolio_id: number; name: string; }

interface ImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accounts: Account[];
  onImported: () => void;
}

type Step = "pick" | "account" | "preview" | "fix" | "importing" | "done";
type ImportSource = "ANGELONE" | "CHOICE_MF" | "CHOICE_EQUITY" | "ICICI_EQUITY";

interface AngelOneParsed {
  trades: AngelOneTrade[];
  unmatched_scrips: string[];
  date_range: [string, string];
  client_code: string;
}
interface AngelOneTrade {
  trade_date: string;
  scrip_name: string;
  side: string;
  price_rs: number;
  quantity: number;
  order_type: string;
  trade_id: string;
  total_charges_rs: number;
}

interface ChoiceMfParsed {
  transactions: ChoiceMfTxn[];
  total_rows: number;
  skipped_rows: number;
}
interface ChoiceMfTxn {
  trade_date: string;
  scheme_name: string;
  txn_type: string;
  units: number;
  nav_rs: number;
  amount_rs: number;
  isin: string;
}

interface SkippedRow {
  security_name: string;
  raw_date: string;
  buy_qty: string;
  buy_price: string;
  sell_qty: string;
  sell_price: string;
  reason: string;
}

interface ChoiceEquityParsed {
  transactions: ChoiceEquityTrade[];
  total_rows: number;
  skipped_rows: number;
  intraday_rows: number;
  client_id?: string;
  client_name?: string;
  skipped_details: SkippedRow[];
}
interface ChoiceEquityTrade {
  trade_date: string;
  security_name: string;
  exchange_code: string;
  txn_type: string;
  quantity: number;
  price: number;
}

interface IciciEquityParsed {
  transactions: IciciEquityTrade[];
  contract_charges: IciciContractNoteCharges[];
  total_rows: number;
  skipped_rows: number;
  client_code: string;
  date_range: [string, string];
}
interface IciciEquityTrade {
  trade_date: string;
  trade_time?: string;
  cn_no: string;
  security_name: string;
  isin: string;
  exchange: string;
  txn_type: string;
  quantity: number;
  price: number;
  brokerage: number;
  broker_ref?: string;
}
interface IciciContractNoteCharges {
  cn_no: string;
  trade_date: string;
  stt_paise: number;
  stamp_charges_paise: number;
  gst_paise: number;
  trans_charges_paise: number;
  other_charges_paise: number;
  total_payable_paise: number;
}

type AnyParsed = AngelOneParsed | ChoiceMfParsed | ChoiceEquityParsed | IciciEquityParsed;

// ─── Source config ────────────────────────────────────────────────────────────

interface ImportSourceMeta { value: ImportSource; label: string; description: string; }

function prefilledAccount(source: ImportSource, parsed: AnyParsed) {
  if (source === "ANGELONE") {
    const p = parsed as AngelOneParsed;
    return { name: p.client_code || "Angel One", accountType: "EQUITY", broker: "Angel One", accountNo: p.client_code || "" };
  }
  if (source === "CHOICE_EQUITY") {
    const p = parsed as ChoiceEquityParsed;
    return {
      name: p.client_name || "Choice Equity",
      accountType: "EQUITY",
      broker: "Choice Equity",
      accountNo: p.client_id || "",
    };
  }
  if (source === "ICICI_EQUITY") {
    const p = parsed as IciciEquityParsed;
    return {
      name: p.client_code || "ICICI Securities",
      accountType: "DEMAT",
      broker: "ICICI Securities",
      accountNo: p.client_code || "",
    };
  }
  return { name: "Choice MF", accountType: "MF_FOLIO", broker: "Choice Wealth", accountNo: "" };
}

function detectExistingAccount(source: ImportSource, parsed: AnyParsed, accounts: Account[]) {
  if (source === "ANGELONE") {
    const code = (parsed as AngelOneParsed).client_code;
    if (code) return accounts.find(a => a.account_no === code || a.name === code) ?? null;
  }
  if (source === "CHOICE_EQUITY") {
    const id = (parsed as ChoiceEquityParsed).client_id;
    if (id) return accounts.find(a => a.account_no === id) ?? null;
  }
  if (source === "ICICI_EQUITY") {
    const code = (parsed as IciciEquityParsed).client_code;
    if (code) return accounts.find(a => a.account_no === code) ?? null;
  }
  return null;
}

// ─── Component ────────────────────────────────────────────────────────────────

export function ImportDialog({ open: isOpen, onOpenChange, accounts, onImported }: ImportDialogProps) {
  const [step, setStep]           = useState<Step>("pick");
  const [source, setSource]       = useState<ImportSource | "">("");
  const [filePaths, setFilePaths] = useState<string[]>([]);
  const [parsing, setParsing]     = useState(false);
  const [parseError, setParseError] = useState("");

  const [parsed, setParsed] = useState<AnyParsed | null>(null);

  // Account resolution
  const [accountMode, setAccountMode]   = useState<"existing" | "create">("create");
  const [selectedAcctId, setSelectedAcctId] = useState("");
  const [sources, setSources]           = useState<ImportSourceMeta[]>([]);
  const [portfolios, setPortfolios]     = useState<Portfolio[]>([]);
  const [newAcct, setNewAcct] = useState({ name: "", portfolioId: "", accountType: "EQUITY", broker: "", accountNo: "" });
  const [creatingPortfolio, setCreatingPortfolio] = useState(false);
  const [newPortfolioName, setNewPortfolioName]   = useState("");
  const [acctError, setAcctError]       = useState("");
  const [savingAcct, setSavingAcct]     = useState(false);
  const [resolvedAcct, setResolvedAcct] = useState<Account | null>(null);

  // Editable skipped rows (fix-issues step)
  interface FixableRow extends SkippedRow {
    ignore: boolean;
    fixed_date: string;
    fixed_txn_type: string;
    fixed_qty: string;
    fixed_price: string;
  }
  const [fixableRows, setFixableRows] = useState<FixableRow[]>([]);

  // Import result
  interface SkippedDetail { trade_date: string; security_name: string; txn_type: string; quantity: number; price: number; reason: string; }
  const [importResult, setImportResult] = useState<{ imported: number; skipped: number; unmatched?: string[]; autoCreated?: string[]; skippedDetails?: SkippedDetail[] } | null>(null);
  const [importError, setImportError]   = useState("");

  // ── Load portfolios once ──────────────────────────────────────────────────
  useEffect(() => {
    if (isOpen) {
      invoke<Portfolio[]>("get_portfolios").then(setPortfolios).catch(() => {});
      invoke<ImportSourceMeta[]>("get_import_sources").then(setSources).catch(() => {});
    }
  }, [isOpen]);

  // ── Reset ─────────────────────────────────────────────────────────────────
  const reset = () => {
    setStep("pick"); setSource(""); setFilePaths([]); setParsing(false);
    setParseError(""); setParsed(null);
    setAccountMode("create"); setSelectedAcctId(""); setResolvedAcct(null);
    setNewAcct({ name: "", portfolioId: "", accountType: "EQUITY", broker: "", accountNo: "" });
    setAcctError(""); setSavingAcct(false);
    setImportResult(null); setImportError("");
  };

  // ── Step 1: pick files ────────────────────────────────────────────────────
  const handlePickFile = async () => {
    const selected = await open({ title: "Select Statement File(s)", multiple: true });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setFilePaths(prev => {
      // Deduplicate by filename
      const existing = new Set(prev);
      return [...prev, ...paths.filter(p => !existing.has(p))];
    });
  };

  const removeFile = (path: string) => setFilePaths(prev => prev.filter(p => p !== path));

  const handleParse = async () => {
    if (!source || filePaths.length === 0) return;
    setParsing(true);
    setParseError("");
    try {
      // Parse each file and merge results
      let result: AnyParsed;
      if (source === "ANGELONE") {
        const results = await Promise.all(
          filePaths.map(fp => invoke<AngelOneParsed>("parse_angel_one_xlsx", { filePath: fp }))
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
          filePaths.map(fp => invoke<ChoiceMfParsed>("parse_choice_mf_pdf", { filePath: fp }))
        );
        result = results.slice(1).reduce((acc, r) => ({
          transactions: [...acc.transactions, ...r.transactions],
          total_rows:   acc.total_rows + r.total_rows,
          skipped_rows: acc.skipped_rows + r.skipped_rows,
        }), results[0]);
      } else if (source === "ICICI_EQUITY") {
        const results = await Promise.all(
          filePaths.map(fp => invoke<IciciEquityParsed>("parse_icici_equity_pdf", { filePath: fp }))
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
        // Deduplicate across files by cn_no: same contract note = same trading day,
        // keep only the first occurrence of each CN (trades + charges).
        const seenCn = new Set<string>();
        merged.transactions = merged.transactions.filter(t => {
          if (seenCn.has(t.cn_no)) return false;
          // Don't add cn_no to seenCn here — multiple trades share the same CN
          return true;
        });
        // Dedup contract_charges by cn_no
        const seenCnCharges = new Set<string>();
        merged.contract_charges = merged.contract_charges.filter(c => {
          if (seenCnCharges.has(c.cn_no)) return false;
          seenCnCharges.add(c.cn_no);
          return true;
        });
        // Dedup trades by broker_ref within the surviving set
        const seenRef = new Set<string>();
        merged.transactions = merged.transactions.filter(t => {
          if (!t.broker_ref) return true;
          if (seenRef.has(t.broker_ref)) return false;
          seenRef.add(t.broker_ref);
          return true;
        });
        merged.total_rows = merged.transactions.length;
        result = merged;
      } else {
        const results = await Promise.all(
          filePaths.map(fp => invoke<ChoiceEquityParsed>("parse_choice_equity_pdf", { filePath: fp }))
        );
        result = results.slice(1).reduce((acc, r) => ({
          transactions:  [...acc.transactions, ...r.transactions],
          total_rows:    acc.total_rows + r.total_rows,
          skipped_rows:  acc.skipped_rows + r.skipped_rows,
          intraday_rows: (acc.intraday_rows ?? 0) + (r.intraday_rows ?? 0),
          client_id:     acc.client_id || r.client_id,
          client_name:   acc.client_name || r.client_name,
          skipped_details: [...(acc.skipped_details ?? []), ...(r.skipped_details ?? [])],
        }), results[0]);
      }
      setParsed(result);

      // Auto-detect existing account
      const existing = detectExistingAccount(source, result, accounts);
      if (existing) {
        setAccountMode("existing");
        setSelectedAcctId(existing.account_id.toString());
        // Pre-fill create form with the matched account's details so switching
        // modes keeps the name consistent with what's already in the system.
        setNewAcct(a => ({
          ...a,
          name: existing.name,
          accountType: existing.account_type,
          broker: existing.broker || "",
          accountNo: existing.account_no || "",
          portfolioId: portfolios[0]?.portfolio_id.toString() ?? "",
        }));
      } else {
        // No auto-match — if existing accounts exist, show the list first so user
        // consciously decides before we default to creating a new one.
        if (accounts.length > 0) {
          setAccountMode("existing");
        } else {
          setAccountMode("create");
        }
        const pre = prefilledAccount(source, result);
        setNewAcct(a => ({ ...a, ...pre, portfolioId: portfolios[0]?.portfolio_id.toString() ?? "" }));
      }

      // If any rows were skipped, go to fix step first
      if (source === "CHOICE_EQUITY") {
        const eq = result as ChoiceEquityParsed;
        if (eq.skipped_details?.length > 0) {
          setFixableRows(eq.skipped_details.map(r => ({
            ...r,
            ignore: false,
            fixed_date: r.raw_date,
            fixed_txn_type: r.sell_qty && r.sell_qty !== "-" && r.sell_qty !== "0" ? "SELL" : "BUY",
            fixed_qty: r.sell_qty && r.sell_qty !== "-" && r.sell_qty !== "0" ? r.sell_qty : r.buy_qty,
            fixed_price: r.sell_price && r.sell_price !== "-" && r.sell_price !== "0.00" ? r.sell_price : r.buy_price,
          })));
          setStep("fix");
          return;
        }
      }
      setStep("account");
    } catch (e: any) {
      setParseError(typeof e === "string" ? e : e?.message ?? "Parse failed");
    } finally {
      setParsing(false);
    }
  };

  // ── Step fix: apply corrections and proceed ───────────────────────────────
  const handleApplyFixes = () => {
    if (!parsed || source !== "CHOICE_EQUITY") { setStep("account"); return; }
    const eq = parsed as ChoiceEquityParsed;
    const added: ChoiceEquityTrade[] = fixableRows
      .filter(r => !r.ignore)
      .map(r => ({
        trade_date: r.fixed_date,
        security_name: r.security_name,
        exchange_code: "",
        txn_type: r.fixed_txn_type,
        quantity: parseFloat(r.fixed_qty.replace(/,/g, "")) || 0,
        price: parseFloat(r.fixed_price.replace(/,/g, "")) || 0,
        trade_segment: "EQ",
      }));
    // Merge fixed rows into the parsed transactions
    const merged = { ...eq, transactions: [...eq.transactions, ...added] };
    setParsed(merged);
    setStep("account");
  };

  // ── Step 2: save / select account ─────────────────────────────────────────
  const handleSaveAccount = async () => {
    if (accountMode === "existing") {
      const acct = accounts.find(a => a.account_id.toString() === selectedAcctId);
      if (!acct) { setAcctError("Select an account"); return; }
      setResolvedAcct(acct);
      setStep("preview");
      return;
    }
    // Create new
    if (!newAcct.name.trim())       { setAcctError("Account name is required"); return; }
    if (!newAcct.portfolioId)       { setAcctError("Select a portfolio"); return; }
    if (!newAcct.accountType)       { setAcctError("Select account type"); return; }
    setSavingAcct(true);
    setAcctError("");
    try {
      const created = await invoke<Account>("create_account", {
        input: {
          portfolio_id: parseInt(newAcct.portfolioId),
          name: newAcct.name.trim(),
          account_type: newAcct.accountType,
          broker: newAcct.broker || null,
          account_no: newAcct.accountNo || null,
        },
      });
      setResolvedAcct(created);
      setStep("preview");
    } catch (e: any) {
      setAcctError(typeof e === "string" ? e : e?.message ?? "Failed to create account");
    } finally {
      setSavingAcct(false);
    }
  };

  // ── Create portfolio inline ───────────────────────────────────────────────
  const handleCreatePortfolio = async () => {
    const name = newPortfolioName.trim();
    if (!name) return;
    try {
      const created = await invoke<Portfolio>("create_portfolio", { input: { name } });
      setPortfolios(ps => [...ps, created]);
      setNewAcct(a => ({ ...a, portfolioId: created.portfolio_id.toString() }));
      setCreatingPortfolio(false);
      setNewPortfolioName("");
    } catch (e: any) {
      setAcctError(typeof e === "string" ? e : e?.message ?? "Failed to create portfolio");
    }
  };

  // ── Step 3: import ────────────────────────────────────────────────────────
  const handleImport = async () => {
    if (!resolvedAcct || !parsed) return;
    setStep("importing");
    setImportError("");
    try {
      if (source === "ANGELONE") {
        const r = await invoke<{ imported: number; skipped: number; auto_created_instruments: string[] }>(
          "import_angel_one_trades",
          { accountId: resolvedAcct.account_id, trades: (parsed as AngelOneParsed).trades },
        );
        setImportResult({ imported: r.imported, skipped: r.skipped, autoCreated: r.auto_created_instruments });
      } else if (source === "CHOICE_MF") {
        const r = await invoke<{ imported: number; skipped: number; auto_created_instruments: number }>(
          "import_choice_mf_transactions",
          { accountId: resolvedAcct.account_id, transactions: (parsed as ChoiceMfParsed).transactions },
        );
        setImportResult({ imported: r.imported, skipped: r.skipped });
      } else if (source === "ICICI_EQUITY") {
        const icici = parsed as IciciEquityParsed;
        const r = await invoke<{ imported: number; skipped: number; contract_notes_imported: number; contract_notes_skipped: number; auto_created_instruments: number; skipped_details: SkippedDetail[] }>(
          "import_icici_equity_trades",
          {
            accountId:       resolvedAcct.account_id,
            transactions:    icici.transactions,
            contractCharges: icici.contract_charges,
            filePaths:       filePaths,
          },
        );
        setImportResult({ imported: r.imported, skipped: r.skipped, skippedDetails: r.skipped_details });
      } else {
        const r = await invoke<{ imported: number; skipped: number; auto_created_instruments: number; skipped_details: SkippedDetail[] }>(
          "import_choice_equity_trades",
          { accountId: resolvedAcct.account_id, transactions: (parsed as ChoiceEquityParsed).transactions },
        );
        setImportResult({ imported: r.imported, skipped: r.skipped, skippedDetails: r.skipped_details });
      }
      setStep("done");
    } catch (e: any) {
      setImportError(typeof e === "string" ? e : e?.message ?? "Import failed");
      setStep("preview");
    }
  };

  // ── Preview rows ──────────────────────────────────────────────────────────
  const previewRows = () => {
    if (!parsed) return null;
    if (source === "ANGELONE") {
      const trades = (parsed as AngelOneParsed).trades.slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th>
            <th className="py-1 text-left">Scrip</th>
            <th className="py-1 text-left">Side</th>
            <th className="py-1 text-right">Price</th>
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
            <th className="py-1 text-left">Date</th>
            <th className="py-1 text-left">Scheme</th>
            <th className="py-1 text-left">Type</th>
            <th className="py-1 text-right">Units</th>
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
    if (source === "CHOICE_EQUITY") {
      const trades = (parsed as ChoiceEquityParsed).transactions.slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th>
            <th className="py-1 text-left">Security</th>
            <th className="py-1 text-left">Side</th>
            <th className="py-1 text-right">Qty</th>
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
    }
    if (source === "ICICI_EQUITY") {
      const trades = (parsed as IciciEquityParsed).transactions.slice(0, 10);
      return (
        <table className="w-full text-xs">
          <thead><tr className="border-b text-muted-foreground">
            <th className="py-1 text-left">Date</th>
            <th className="py-1 text-left">Security</th>
            <th className="py-1 text-left">Side</th>
            <th className="py-1 text-right">Qty</th>
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
    }
    return null;
  };

  const totalRows = parsed
    ? source === "ANGELONE"
      ? (parsed as AngelOneParsed).trades.length
      : source === "CHOICE_EQUITY"
        ? (parsed as ChoiceEquityParsed).total_rows
        : source === "ICICI_EQUITY"
          ? (parsed as IciciEquityParsed).total_rows
          : (parsed as ChoiceMfParsed).total_rows
    : 0;

  // ── Render ────────────────────────────────────────────────────────────────
  return (
    <Dialog open={isOpen} onOpenChange={(o) => { if (!o) reset(); onOpenChange(o); }}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {step === "pick"      && "Import Statement"}
            {step === "account"   && "Account"}
            {step === "fix"       && "Fix Parsing Issues"}
            {step === "preview"   && "Preview & Import"}
            {step === "importing" && "Importing…"}
            {step === "done"      && "Import Complete"}
          </DialogTitle>
          <DialogDescription>
            {step === "pick"    && "Select the source and upload your statement file."}
            {step === "account" && "Confirm or create the account for this statement."}
            {step === "fix"     && "Some rows could not be parsed. Correct them before continuing."}
            {step === "preview" && `${filePaths.length > 1 ? `${filePaths.length} files · ` : ""}${totalRows} transactions found. Review and confirm.`}
            {step === "done"    && "Transactions have been added."}
          </DialogDescription>
        </DialogHeader>

        {/* ── Step: pick ── */}
        {step === "pick" && (
          <div className="space-y-4 py-1">
            <div className="space-y-1.5">
              <Label>Statement source</Label>
              <Select value={source} onValueChange={(v) => { setSource(v as ImportSource); setFilePaths([]); setParseError(""); }}>
                <SelectTrigger><SelectValue placeholder="Select source" /></SelectTrigger>
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

            {source && (
              <div className="space-y-1.5">
                <Label>Statement file{filePaths.length !== 1 ? "s" : ""}</Label>
                {filePaths.length > 0 && (
                  <div className="space-y-1 max-h-[170px] overflow-y-auto pr-0.5">
                    {filePaths.map(fp => (
                      <div key={fp} className="flex items-center gap-2 border rounded-md px-3 py-1.5 bg-muted/30">
                        <span className="flex-1 text-sm truncate">{fp.split("/").pop()}</span>
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
                )}
                <Button variant="outline" onClick={handlePickFile} type="button" className="w-full">
                  {filePaths.length > 0 ? "Add more files" : "Browse…"}
                </Button>
              </div>
            )}

            {parseError && (
              <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
                {parseError}
              </div>
            )}
          </div>
        )}

        {/* ── Step: account ── */}
        {step === "account" && (
          <div className="space-y-4 py-1">
            {/* Prompt when no account was auto-matched */}
            {accountMode === "create" && accounts.length > 0 && (
              <div className="flex gap-2 items-start rounded-md bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 px-3 py-2.5 text-sm">
                <div>
                  <p className="font-medium text-amber-700 dark:text-amber-400">No existing account matched this statement.</p>
                  <p className="text-xs text-amber-600 dark:text-amber-500 mt-0.5">
                    If you have already imported this broker before, select the existing account instead of creating a new one.
                  </p>
                </div>
              </div>
            )}
            {/* Tabs: use existing / create new */}
            <div className="flex gap-2">
              <Button
                size="sm"
                variant={accountMode === "existing" ? "default" : "outline"}
                onClick={() => setAccountMode("existing")}
                disabled={accounts.length === 0}
              >
                Use existing {accounts.length > 0 && `(${accounts.length})`}
              </Button>
              <Button
                size="sm"
                variant={accountMode === "create" ? "default" : "outline"}
                onClick={() => setAccountMode("create")}
              >
                Create new
              </Button>
            </div>

            {accountMode === "existing" ? (
              <div className="space-y-1.5">
                <Label>Account</Label>
                <Select value={selectedAcctId} onValueChange={(v) => setSelectedAcctId(v ?? "")}>
                  <SelectTrigger><SelectValue placeholder="Select account" /></SelectTrigger>
                  <SelectContent>
                    {accounts.map(a => (
                      <SelectItem key={a.account_id} value={a.account_id.toString()}>
                        {a.name} {a.broker ? `· ${a.broker}` : ""}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ) : (
              <div className="space-y-3">
                <div className="space-y-1.5">
                  <Label>Account name <span className="text-muted-foreground text-xs">(display name)</span></Label>
                  <Input value={newAcct.name} onChange={e => setNewAcct(a => ({ ...a, name: e.target.value }))} placeholder="e.g. ICICI Trading" />
                </div>
                <div className="space-y-1.5">
                  <Label>Portfolio</Label>
                  {creatingPortfolio ? (
                    <div className="flex gap-2">
                      <Input
                        autoFocus
                        placeholder="Portfolio name (e.g. Wife, Father)"
                        value={newPortfolioName}
                        onChange={e => setNewPortfolioName(e.target.value)}
                        onKeyDown={async e => {
                          if (e.key === "Enter") await handleCreatePortfolio();
                          if (e.key === "Escape") { setCreatingPortfolio(false); setNewPortfolioName(""); }
                        }}
                      />
                      <Button size="sm" onClick={handleCreatePortfolio} disabled={!newPortfolioName.trim()}>Add</Button>
                      <Button size="sm" variant="outline" onClick={() => { setCreatingPortfolio(false); setNewPortfolioName(""); }}>✕</Button>
                    </div>
                  ) : (
                    <Select value={newAcct.portfolioId} onValueChange={v => {
                      if (v === "__new__") { setCreatingPortfolio(true); return; }
                      setNewAcct(a => ({ ...a, portfolioId: v ?? "" }));
                    }}>
                      <SelectTrigger><SelectValue placeholder="Select portfolio" /></SelectTrigger>
                      <SelectContent>
                        {portfolios.map(p => (
                          <SelectItem key={p.portfolio_id} value={p.portfolio_id.toString()}>{p.name}</SelectItem>
                        ))}
                        <SelectItem value="__new__" className="text-primary font-medium">+ Create new portfolio</SelectItem>
                      </SelectContent>
                    </Select>
                  )}
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div className="space-y-1.5">
                    <Label>Type</Label>
                    <Select value={newAcct.accountType} onValueChange={v => setNewAcct(a => ({ ...a, accountType: v ?? "EQUITY" }))}>
                      <SelectTrigger><SelectValue /></SelectTrigger>
                      <SelectContent>
                        <SelectItem value="EQUITY">Equity / Demat</SelectItem>
                        <SelectItem value="MF">Mutual Fund</SelectItem>
                        <SelectItem value="FD">Fixed Deposit</SelectItem>
                        <SelectItem value="OTHER">Other</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-1.5">
                    <Label>Broker / AMC</Label>
                    <Input value={newAcct.broker} onChange={e => setNewAcct(a => ({ ...a, broker: e.target.value }))} placeholder="e.g. Angel One" />
                  </div>
                </div>
                <div className="space-y-1.5">
                  <Label>Client ID <span className="text-muted-foreground text-xs">(optional)</span></Label>
                  <Input
                    list="existing-client-ids"
                    value={newAcct.accountNo}
                    onChange={e => {
                      const val = e.target.value;
                      setNewAcct(a => ({ ...a, accountNo: val }));
                      // If this matches an existing account, switch to existing mode
                      const match = accounts.find(a => a.account_no === val);
                      if (match) {
                        setAccountMode("existing");
                        setSelectedAcctId(match.account_id.toString());
                      }
                    }}
                    placeholder="e.g. A1234567"
                  />
                  <datalist id="existing-client-ids">
                    {accounts.filter(a => a.account_no).map(a => (
                      <option key={a.account_id} value={a.account_no!}>{a.name}</option>
                    ))}
                  </datalist>
                </div>
              </div>
            )}

            {acctError && (
              <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
                {acctError}
              </div>
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
                        <Input
                          className="h-7 text-xs"
                          value={r.fixed_date}
                          placeholder="YYYY-MM-DD"
                          onChange={e => setFixableRows(rows => rows.map((row, j) =>
                            j === i ? { ...row, fixed_date: e.target.value } : row
                          ))}
                        />
                      </div>
                      <div className="space-y-1">
                        <label className="text-xs text-muted-foreground">Type</label>
                        <select
                          className="w-full h-7 text-xs rounded-md border border-input bg-background px-2"
                          value={r.fixed_txn_type}
                          onChange={e => setFixableRows(rows => rows.map((row, j) =>
                            j === i ? { ...row, fixed_txn_type: e.target.value } : row
                          ))}
                        >
                          <option value="BUY">BUY</option>
                          <option value="SELL">SELL</option>
                        </select>
                      </div>
                      <div className="space-y-1">
                        <label className="text-xs text-muted-foreground">Quantity</label>
                        <Input
                          className="h-7 text-xs"
                          value={r.fixed_qty}
                          onChange={e => setFixableRows(rows => rows.map((row, j) =>
                            j === i ? { ...row, fixed_qty: e.target.value } : row
                          ))}
                        />
                      </div>
                      <div className="space-y-1">
                        <label className="text-xs text-muted-foreground">Price (₹)</label>
                        <Input
                          className="h-7 text-xs"
                          value={r.fixed_price}
                          onChange={e => setFixableRows(rows => rows.map((row, j) =>
                            j === i ? { ...row, fixed_price: e.target.value } : row
                          ))}
                        />
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {/* ── Step: preview ── */}
        {step === "preview" && parsed && (
          <div className="space-y-3 py-1">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <span>Account: <span className="font-medium text-foreground">{resolvedAcct?.name}</span></span>
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
            {source === "CHOICE_EQUITY" && (parsed as ChoiceEquityParsed).intraday_rows > 0 && (
              <div className="text-xs text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-md px-3 py-2">
                {(parsed as ChoiceEquityParsed).intraday_rows} intraday round-trip(s) detected — imported as both BUY and SELL legs, classified as speculative income in the tax report.
              </div>
            )}
            {importError && (
              <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
                {importError}
              </div>
            )}
          </div>
        )}

        {/* ── Step: importing ── */}
        {step === "importing" && (
          <div className="py-6 text-center text-sm text-muted-foreground">Importing transactions…</div>
        )}

        {/* ── Step: done ── */}
        {step === "done" && importResult && (
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
                <div className="text-xs font-medium text-muted-foreground">
                  {importResult.skippedDetails.length} skipped
                </div>
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

          {step === "pick" && (
            <Button onClick={handleParse} disabled={!source || filePaths.length === 0 || parsing}>
              {parsing ? "Parsing…" : filePaths.length > 1 ? `Parse ${filePaths.length} Files` : "Parse File"}
            </Button>
          )}

          {step === "fix" && (
            <>
              <Button variant="outline" onClick={() => setStep("pick")}>Back</Button>
              <Button onClick={handleApplyFixes}>
                Continue with {fixableRows.filter(r => !r.ignore).length} fix{fixableRows.filter(r => !r.ignore).length !== 1 ? "es" : ""}
              </Button>
            </>
          )}

          {step === "account" && (
            <>
              <Button variant="outline" onClick={() => setStep("pick")}>Back</Button>
              <Button onClick={handleSaveAccount} disabled={savingAcct}>
                {savingAcct ? "Saving…" : "Continue"}
              </Button>
            </>
          )}

          {step === "preview" && (
            <>
              <Button variant="outline" onClick={() => setStep("account")}>Back</Button>
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
