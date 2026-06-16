import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { ACCOUNT_TYPES, BROKERS } from "@/lib/account-types";

// ─── Types ────────────────────────────────────────────────────────────────────

export interface Person    { person_id: number; name: string; pan?: string; }
export interface Portfolio { portfolio_id: number; person_id?: number; name: string; }
export interface Account   { account_id: number; portfolio_id: number; name: string; account_type: string; broker?: string; account_no?: string; }

export interface Selection {
  person:    Person;
  portfolio: Portfolio;
  account:   Account;
}

interface Props {
  persons:    Person[];
  portfolios: Portfolio[];
  accounts:   Account[];
  value:      Partial<Selection>;
  onChange:   (sel: Partial<Selection>) => void;
  onPersonCreated?:    (p: Person)    => void;
  onPortfolioCreated?: (p: Portfolio) => void;
  onAccountCreated?:   (a: Account)   => void;
  showAccount?: boolean;
  error?: string;
}

// ─── Component ────────────────────────────────────────────────────────────────

export function PersonPortfolioAccountSelector({
  persons, portfolios, accounts,
  value, onChange,
  onPersonCreated, onPortfolioCreated, onAccountCreated,
  showAccount = true,
  error,
}: Props) {
  // Inline create person
  const [creatingPerson,  setCreatingPerson]  = useState(false);
  const [newPersonName,   setNewPersonName]   = useState("");
  const [newPersonPan,    setNewPersonPan]    = useState("");
  const [personError,     setPersonError]     = useState("");

  // Inline create portfolio
  const [creatingPortfolio, setCreatingPortfolio] = useState(false);
  const [newPortfolioName,  setNewPortfolioName]   = useState("");
  const [portfolioError,    setPortfolioError]     = useState("");

  // Inline create account
  const [creatingAccount,    setCreatingAccount]    = useState(false);
  const [newAccountName,     setNewAccountName]     = useState("");
  const [newAccountType,     setNewAccountType]     = useState("");
  const [newAccountBroker,   setNewAccountBroker]   = useState("");
  const [newAccountNo,       setNewAccountNo]       = useState("");
  const [accountError,       setAccountError]       = useState("");

  const [saving, setSaving] = useState(false);

  // Filtered lists based on current selection
  const filteredPortfolios = value.person
    ? portfolios.filter(p => p.person_id === value.person!.person_id)
    : portfolios;

  const filteredAccounts = value.portfolio
    ? accounts.filter(a => a.portfolio_id === value.portfolio!.portfolio_id)
    : accounts;

  const selectedAccountType = ACCOUNT_TYPES.find(t => t.value === newAccountType);

  // ── Handlers ─────────────────────────────────────────────────────────────

  const handlePersonChange = (personId: string | null) => {
    if (!personId) return;
    if (personId === "__new__") { setCreatingPerson(true); return; }
    const person = persons.find(p => p.person_id.toString() === personId) ?? null;
    onChange({ person: person ?? undefined, portfolio: undefined, account: undefined });
  };

  const handlePortfolioChange = (portfolioId: string | null) => {
    if (!portfolioId) return;
    if (portfolioId === "__new__") { setCreatingPortfolio(true); return; }
    const portfolio = portfolios.find(p => p.portfolio_id.toString() === portfolioId) ?? null;
    onChange({ ...value, portfolio: portfolio ?? undefined, account: undefined });
  };

  const handleAccountChange = (accountId: string | null) => {
    if (!accountId) return;
    if (accountId === "__new__") { setCreatingAccount(true); return; }
    const account = accounts.find(a => a.account_id.toString() === accountId) ?? null;
    onChange({ ...value, account: account ?? undefined });
  };

  // ── Inline create person ─────────────────────────────────────────────────

  const handleCreatePerson = async () => {
    const name = newPersonName.trim();
    if (!name) { setPersonError("Name is required"); return; }
    setSaving(true);
    setPersonError("");
    try {
      const created = await invoke<Person>("create_person", {
        input: { name, pan: newPersonPan.trim() || null },
      });
      onPersonCreated?.(created);
      onChange({ person: created, portfolio: undefined, account: undefined });
      setCreatingPerson(false);
      setNewPersonName("");
      setNewPersonPan("");
    } catch (e: any) {
      setPersonError(typeof e === "string" ? e : e?.message ?? "Failed");
    } finally {
      setSaving(false);
    }
  };

  // ── Inline create portfolio ───────────────────────────────────────────────

  const handleCreatePortfolio = async () => {
    const name = newPortfolioName.trim();
    if (!name) { setPortfolioError("Name is required"); return; }
    if (!value.person) { setPortfolioError("Select a person first"); return; }
    setSaving(true);
    setPortfolioError("");
    try {
      const created = await invoke<Portfolio>("create_portfolio", {
        input: { name, person_id: value.person.person_id },
      });
      onPortfolioCreated?.(created);
      onChange({ ...value, portfolio: created, account: undefined });
      setCreatingPortfolio(false);
      setNewPortfolioName("");
    } catch (e: any) {
      setPortfolioError(typeof e === "string" ? e : e?.message ?? "Failed");
    } finally {
      setSaving(false);
    }
  };

  // ── Inline create account ─────────────────────────────────────────────────

  const handleCreateAccount = async () => {
    const name = newAccountName.trim();
    if (!name) { setAccountError("Account name is required"); return; }
    if (!newAccountType) { setAccountError("Select account type"); return; }
    if (!value.portfolio) { setAccountError("Select a portfolio first"); return; }
    setSaving(true);
    setAccountError("");
    try {
      const accountType = ACCOUNT_TYPES.find(t => t.value === newAccountType)!;
      const created = await invoke<Account>("create_account", {
        input: {
          portfolio_id: value.portfolio.portfolio_id,
          name,
          account_type: newAccountType,
          broker: accountType.brokerRequired && newAccountBroker ? newAccountBroker : null,
          account_no: newAccountNo.trim() || null,
        },
      });
      onAccountCreated?.(created);
      onChange({ ...value, account: created });
      setCreatingAccount(false);
      setNewAccountName("");
      setNewAccountType("");
      setNewAccountBroker("");
      setNewAccountNo("");
    } catch (e: any) {
      setAccountError(typeof e === "string" ? e : e?.message ?? "Failed");
    } finally {
      setSaving(false);
    }
  };

  const cancelCreateAccount = () => {
    setCreatingAccount(false);
    setNewAccountName("");
    setNewAccountType("");
    setNewAccountBroker("");
    setNewAccountNo("");
    setAccountError("");
  };

  // ── Render ────────────────────────────────────────────────────────────────

  return (
    <div className="space-y-3">
      {/* Person */}
      <div className="space-y-1.5">
        <Label>Person</Label>
        {creatingPerson ? (
          <div className="space-y-2">
            <div className="flex gap-2">
              <Input
                autoFocus
                placeholder="Name (e.g. Rahul)"
                value={newPersonName}
                onChange={e => setNewPersonName(e.target.value)}
                onKeyDown={e => { if (e.key === "Enter") handleCreatePerson(); if (e.key === "Escape") { setCreatingPerson(false); setNewPersonName(""); setNewPersonPan(""); } }}
              />
              <Input
                placeholder="PAN (optional)"
                value={newPersonPan}
                onChange={e => setNewPersonPan(e.target.value.toUpperCase())}
                className="uppercase"
              />
              <Button size="sm" onClick={handleCreatePerson} disabled={!newPersonName.trim() || saving}>
                {saving ? "…" : "Add"}
              </Button>
              <Button size="sm" variant="outline" onClick={() => { setCreatingPerson(false); setNewPersonName(""); setNewPersonPan(""); }}>✕</Button>
            </div>
            {personError && <p className="text-xs text-destructive">{personError}</p>}
          </div>
        ) : (
          <Select value={value.person?.person_id.toString() ?? ""} onValueChange={handlePersonChange}>
            <SelectTrigger><SelectValue placeholder="Select person" /></SelectTrigger>
            <SelectContent>
              {persons.map(p => (
                <SelectItem key={p.person_id} value={p.person_id.toString()}>
                  {p.name}{p.pan ? ` · ${p.pan}` : ""}
                </SelectItem>
              ))}
              <SelectItem value="__new__" className="text-primary font-medium">+ Add new person</SelectItem>
            </SelectContent>
          </Select>
        )}
      </div>

      {/* Portfolio */}
      <div className="space-y-1.5">
        <Label>Portfolio</Label>
        {creatingPortfolio ? (
          <div className="space-y-2">
            <div className="flex gap-2">
              <Input
                autoFocus
                placeholder="Portfolio name (e.g. Long Term)"
                value={newPortfolioName}
                onChange={e => setNewPortfolioName(e.target.value)}
                onKeyDown={e => { if (e.key === "Enter") handleCreatePortfolio(); if (e.key === "Escape") { setCreatingPortfolio(false); setNewPortfolioName(""); } }}
              />
              <Button size="sm" onClick={handleCreatePortfolio} disabled={!newPortfolioName.trim() || saving}>
                {saving ? "…" : "Add"}
              </Button>
              <Button size="sm" variant="outline" onClick={() => { setCreatingPortfolio(false); setNewPortfolioName(""); }}>✕</Button>
            </div>
            {portfolioError && <p className="text-xs text-destructive">{portfolioError}</p>}
          </div>
        ) : (
          <Select
            value={value.portfolio?.portfolio_id.toString() ?? ""}
            onValueChange={handlePortfolioChange}
            disabled={!value.person}
          >
            <SelectTrigger><SelectValue placeholder={value.person ? "Select portfolio" : "Select a person first"} /></SelectTrigger>
            <SelectContent>
              {filteredPortfolios.map(p => (
                <SelectItem key={p.portfolio_id} value={p.portfolio_id.toString()}>{p.name}</SelectItem>
              ))}
              <SelectItem value="__new__" className="text-primary font-medium" disabled={!value.person}>
                + Create new portfolio
              </SelectItem>
            </SelectContent>
          </Select>
        )}
      </div>

      {/* Account */}
      {showAccount && (
        <div className="space-y-1.5">
          <Label>Account</Label>
          {creatingAccount ? (
            <div className="space-y-2">
              <div className="grid grid-cols-2 gap-2">
                <Input
                  autoFocus
                  placeholder='Account name (e.g. "Zerodha Demat")'
                  value={newAccountName}
                  onChange={e => setNewAccountName(e.target.value)}
                  className="col-span-2"
                  onKeyDown={e => { if (e.key === "Escape") cancelCreateAccount(); }}
                />
                <Select value={newAccountType} onValueChange={(v) => { setNewAccountType(v ?? ""); setNewAccountBroker(""); }}>
                  <SelectTrigger><SelectValue placeholder="Type" /></SelectTrigger>
                  <SelectContent>
                    {ACCOUNT_TYPES.map(t => (
                      <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {selectedAccountType?.brokerRequired ? (
                  <Select value={newAccountBroker} onValueChange={(v) => setNewAccountBroker(v ?? "")}>
                    <SelectTrigger><SelectValue placeholder="Broker" /></SelectTrigger>
                    <SelectContent>
                      {BROKERS.map(b => <SelectItem key={b} value={b}>{b}</SelectItem>)}
                    </SelectContent>
                  </Select>
                ) : (
                  <Input
                    placeholder="Account / folio no. (optional)"
                    value={newAccountNo}
                    onChange={e => setNewAccountNo(e.target.value)}
                  />
                )}
              </div>
              <div className="flex gap-2">
                <Button size="sm" onClick={handleCreateAccount} disabled={!newAccountName.trim() || !newAccountType || saving} className="flex-1">
                  {saving ? "…" : "Add Account"}
                </Button>
                <Button size="sm" variant="outline" onClick={cancelCreateAccount}>✕</Button>
              </div>
              {accountError && <p className="text-xs text-destructive">{accountError}</p>}
            </div>
          ) : (
            <Select
              value={value.account?.account_id.toString() ?? ""}
              onValueChange={handleAccountChange}
              disabled={!value.portfolio}
            >
              <SelectTrigger><SelectValue placeholder={value.portfolio ? "Select account" : "Select a portfolio first"} /></SelectTrigger>
              <SelectContent>
                {filteredAccounts.map(a => (
                  <SelectItem key={a.account_id} value={a.account_id.toString()}>
                    {a.name}{a.broker ? ` · ${a.broker}` : ""}
                  </SelectItem>
                ))}
                <SelectItem value="__new__" className="text-primary font-medium" disabled={!value.portfolio}>
                  + Create new account
                </SelectItem>
              </SelectContent>
            </Select>
          )}
        </div>
      )}

      {error && <p className="text-sm text-destructive">{error}</p>}
    </div>
  );
}
