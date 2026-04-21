import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Plus, Trash2, CheckCircle2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ACCOUNT_TYPES, BROKERS } from "@/lib/account-types";
import { cn } from "@/lib/utils";

interface AccountDraft {
  id: number; // local-only key for list rendering
  name: string;
  account_type: string;
  broker: string;
  account_no: string;
}

interface OnboardingWizardProps {
  onComplete: () => void;
}

type Step = "portfolio" | "accounts" | "done";

let draftId = 0;

export function OnboardingWizard({ onComplete }: OnboardingWizardProps) {
  const [step, setStep] = useState<Step>("portfolio");
  const [portfolioName, setPortfolioName] = useState("");
  const [portfolioId, setPortfolioId] = useState<number | null>(null);
  const [accounts, setAccounts] = useState<AccountDraft[]>([newAccountDraft()]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  function newAccountDraft(): AccountDraft {
    return { id: ++draftId, name: "", account_type: "", broker: "", account_no: "" };
  }

  const addAccount = () => setAccounts((a) => [...a, newAccountDraft()]);

  const removeAccount = (id: number) =>
    setAccounts((a) => a.filter((x) => x.id !== id));

  const updateAccount = (id: number, patch: Partial<AccountDraft>) =>
    setAccounts((a) => a.map((x) => (x.id === id ? { ...x, ...patch } : x)));

  // Step 1: create portfolio
  const handleCreatePortfolio = async () => {
    const name = portfolioName.trim();
    if (!name) { setError("Enter a portfolio name"); return; }
    setSaving(true);
    setError("");
    try {
      const p = await invoke<{ portfolio_id: number }>("create_portfolio", { input: { name } });
      setPortfolioId(p.portfolio_id);
      setStep("accounts");
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setSaving(false);
    }
  };

  // Step 2: create accounts (optional — skip if none filled in)
  const handleSaveAccounts = async () => {
    const valid = accounts.filter((a) => a.name.trim() && a.account_type);
    if (valid.length === 0) { setStep("done"); return; }

    setSaving(true);
    setError("");
    try {
      for (const a of valid) {
        const type = ACCOUNT_TYPES.find((t) => t.value === a.account_type)!;
        await invoke("create_account", {
          input: {
            portfolio_id: portfolioId,
            name: a.name.trim(),
            account_type: a.account_type,
            broker: type.brokerRequired && a.broker ? a.broker : null,
            account_no: a.account_no.trim() || null,
          },
        });
      }
      setStep("done");
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-4">
      {/* Progress indicator */}
      <div className="w-full max-w-xl space-y-6">
        <div className="flex items-center gap-2 justify-center">
          {(["portfolio", "accounts", "done"] as Step[]).map((s, i) => (
            <div key={s} className="flex items-center gap-2">
              <div className={cn(
                "size-7 rounded-full flex items-center justify-center text-xs font-semibold border-2 transition-colors",
                step === s
                  ? "bg-primary text-primary-foreground border-primary"
                  : ["portfolio", "accounts", "done"].indexOf(step) > i
                    ? "bg-primary/20 text-primary border-primary/40"
                    : "bg-muted text-muted-foreground border-border"
              )}>
                {i + 1}
              </div>
              {i < 2 && <div className="w-8 h-px bg-border" />}
            </div>
          ))}
        </div>

        {/* Step 1 — Portfolio name */}
        {step === "portfolio" && (
          <Card>
            <CardHeader>
              <CardTitle>Create your first portfolio</CardTitle>
              <CardDescription>
                A portfolio groups all your accounts together.
                You can create more portfolios later (e.g. "Spouse", "Kids").
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label>Portfolio name</Label>
                <Input
                  placeholder='e.g. "My Portfolio" or "Self"'
                  value={portfolioName}
                  onChange={(e) => setPortfolioName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleCreatePortfolio()}
                  autoFocus
                />
              </div>
              {error && <p className="text-destructive text-sm">{error}</p>}
              <Button
                className="w-full"
                onClick={handleCreatePortfolio}
                disabled={!portfolioName.trim() || saving}
              >
                {saving ? "Creating…" : "Continue"}
              </Button>
            </CardContent>
          </Card>
        )}

        {/* Step 2 — Accounts */}
        {step === "accounts" && (
          <Card>
            <CardHeader>
              <CardTitle>Add your accounts</CardTitle>
              <CardDescription>
                Optionally add accounts now — demat, MF folios, FDs, etc.
                You can also skip and add them later from Settings.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-3">
                {accounts.map((account, idx) => (
                  <AccountRow
                    key={account.id}
                    account={account}
                    index={idx}
                    onUpdate={(patch) => updateAccount(account.id, patch)}
                    onRemove={accounts.length > 1 ? () => removeAccount(account.id) : undefined}
                  />
                ))}
              </div>

              <Button variant="outline" className="w-full" onClick={addAccount}>
                <Plus className="size-4 mr-2" />
                Add another account
              </Button>

              {error && <p className="text-destructive text-sm">{error}</p>}

              <div className="flex gap-3 pt-2">
                <Button
                  variant="outline"
                  className="flex-1"
                  onClick={() => { setStep("portfolio"); setError(""); }}
                >
                  Back
                </Button>
                <Button
                  className="flex-1"
                  onClick={handleSaveAccounts}
                  disabled={saving}
                >
                  {saving ? "Saving…" : accounts.some(a => a.name.trim() && a.account_type) ? "Save & Continue" : "Skip for now"}
                </Button>
              </div>
            </CardContent>
          </Card>
        )}

        {/* Step 3 — Done */}
        {step === "done" && (
          <Card>
            <CardHeader className="text-center">
              <div className="flex justify-center mb-2">
                <CheckCircle2 className="size-12 text-primary" />
              </div>
              <CardTitle>You're all set!</CardTitle>
              <CardDescription>
                Your portfolio has been created.
                Next, add accounts from Settings, then import your broker or CAMS statements.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Button className="w-full" onClick={onComplete}>
                Go to Dashboard
              </Button>
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  );
}

// -----------------------------------------------------------------------
// Account row sub-component
// -----------------------------------------------------------------------
interface AccountRowProps {
  account: AccountDraft;
  index: number;
  onUpdate: (patch: Partial<AccountDraft>) => void;
  onRemove?: () => void;
}

function AccountRow({ account, index, onUpdate, onRemove }: AccountRowProps) {
  const selectedType = ACCOUNT_TYPES.find((t) => t.value === account.account_type);

  return (
    <div className="border rounded-lg p-3 space-y-3 bg-muted/30">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-muted-foreground">Account {index + 1}</span>
        {onRemove && (
          <button onClick={onRemove} className="text-muted-foreground hover:text-destructive transition-colors">
            <Trash2 className="size-4" />
          </button>
        )}
      </div>

      <div className="grid grid-cols-2 gap-3">
        {/* Account name */}
        <div className="space-y-1 col-span-2">
          <Label className="text-xs">Account name</Label>
          <Input
            placeholder='e.g. "Zerodha Demat" or "HDFC MF Folio"'
            value={account.name}
            onChange={(e) => onUpdate({ name: e.target.value })}
          />
        </div>

        {/* Account type */}
        <div className="space-y-1">
          <Label className="text-xs">Type</Label>
          <Select value={account.account_type} onValueChange={(v) => onUpdate({ account_type: v, broker: "" })}>
            <SelectTrigger>
              <SelectValue placeholder="Select type" />
            </SelectTrigger>
            <SelectContent>
              {ACCOUNT_TYPES.map((t) => (
                <SelectItem key={t.value} value={t.value}>
                  <div>
                    <div className="font-medium">{t.label}</div>
                    <div className="text-xs text-muted-foreground">{t.description}</div>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {/* Broker — only for DEMAT */}
        {selectedType?.brokerRequired && (
          <div className="space-y-1">
            <Label className="text-xs">Broker</Label>
            <Select value={account.broker} onValueChange={(v) => onUpdate({ broker: v })}>
              <SelectTrigger>
                <SelectValue placeholder="Select broker" />
              </SelectTrigger>
              <SelectContent>
                {BROKERS.map((b) => (
                  <SelectItem key={b} value={b}>{b}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}

        {/* Account number — optional */}
        <div className={cn("space-y-1", selectedType?.brokerRequired ? "col-span-2" : "col-span-2")}>
          <Label className="text-xs">Account / folio number <span className="text-muted-foreground">(optional)</span></Label>
          <Input
            placeholder="e.g. ZE12345 or MF folio number"
            value={account.account_no}
            onChange={(e) => onUpdate({ account_no: e.target.value })}
          />
        </div>
      </div>
    </div>
  );
}
