import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Plus, Pencil, Trash2, ChevronDown, ChevronRight, Upload, Download } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import { ACCOUNT_TYPES, BROKERS } from "@/lib/account-types";
import { cn } from "@/lib/utils";
import { ExportDialog } from "@/components/sync/ExportDialog";
import { ImportDialog } from "@/components/sync/ImportDialog";

interface Portfolio { portfolio_id: number; name: string; }
interface Account {
  account_id: number; portfolio_id: number; name: string;
  account_type: string; broker?: string; account_no?: string;
}

export function SettingsPage() {
  const [portfolios, setPortfolios] = useState<Portfolio[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [error, setError] = useState("");
  const [showExport, setShowExport] = useState(false);
  const [showImport, setShowImport] = useState(false);

  // Dialog state
  const [newPortfolioName, setNewPortfolioName] = useState("");
  const [showNewPortfolio, setShowNewPortfolio] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{ type: "portfolio" | "account"; id: number; name: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ type: "portfolio" | "account"; id: number; name: string } | null>(null);
  const [newAccount, setNewAccount] = useState<{ portfolioId: number; name: string; type: string; broker: string; accountNo: string } | null>(null);

  const load = async () => {
    const [p, a] = await Promise.all([
      invoke<Portfolio[]>("get_portfolios"),
      invoke<Account[]>("get_accounts", { portfolioId: null }),
    ]);
    setPortfolios(p);
    setAccounts(a);
    // Auto-expand all on first load
    setExpanded(new Set(p.map((x) => x.portfolio_id)));
  };

  useEffect(() => { load(); }, []);

  const toggleExpand = (id: number) =>
    setExpanded((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });

  // Create portfolio
  const handleCreatePortfolio = async () => {
    if (!newPortfolioName.trim()) return;
    try {
      await invoke("create_portfolio", { input: { name: newPortfolioName.trim() } });
      setNewPortfolioName("");
      setShowNewPortfolio(false);
      await load();
    } catch (e: any) { setError(e.toString()); }
  };

  // Rename portfolio or account
  const handleRename = async () => {
    if (!renameTarget) return;
    try {
      if (renameTarget.type === "portfolio") {
        await invoke("rename_portfolio", { portfolioId: renameTarget.id, name: renameTarget.name });
      } else {
        await invoke("rename_account", { accountId: renameTarget.id, name: renameTarget.name });
      }
      setRenameTarget(null);
      await load();
    } catch (e: any) { setError(e.toString()); }
  };

  // Delete portfolio or account
  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      if (deleteTarget.type === "portfolio") {
        await invoke("delete_portfolio", { portfolioId: deleteTarget.id });
      } else {
        await invoke("delete_account", { accountId: deleteTarget.id });
      }
      setDeleteTarget(null);
      await load();
    } catch (e: any) { setError(e.toString()); setDeleteTarget(null); }
  };

  // Create account
  const handleCreateAccount = async () => {
    if (!newAccount || !newAccount.name.trim() || !newAccount.type) return;
    const accountType = ACCOUNT_TYPES.find((t) => t.value === newAccount.type)!;
    try {
      await invoke("create_account", {
        input: {
          portfolio_id: newAccount.portfolioId,
          name: newAccount.name.trim(),
          account_type: newAccount.type,
          broker: accountType.brokerRequired && newAccount.broker ? newAccount.broker : null,
          account_no: newAccount.accountNo.trim() || null,
        },
      });
      setNewAccount(null);
      await load();
    } catch (e: any) { setError(e.toString()); }
  };

  return (
    <div className="space-y-6 max-w-2xl">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Settings</h1>
          <p className="text-muted-foreground text-sm">Manage portfolios, accounts and preferences.</p>
        </div>
      </div>

      {error && (
        <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
          {error}
          <button className="ml-2 underline text-xs" onClick={() => setError("")}>dismiss</button>
        </div>
      )}

      {/* Portfolios & Accounts */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-3">
          <CardTitle className="text-base">Portfolios & Accounts</CardTitle>
          <Button size="sm" variant="outline" onClick={() => setShowNewPortfolio(true)}>
            <Plus className="size-4 mr-1" /> New Portfolio
          </Button>
        </CardHeader>
        <CardContent className="space-y-3">
          {portfolios.length === 0 && (
            <p className="text-sm text-muted-foreground text-center py-4">No portfolios yet.</p>
          )}
          {portfolios.map((portfolio) => {
            const portfolioAccounts = accounts.filter((a) => a.portfolio_id === portfolio.portfolio_id);
            const isExpanded = expanded.has(portfolio.portfolio_id);
            return (
              <div key={portfolio.portfolio_id} className="border rounded-lg overflow-hidden">
                {/* Portfolio row */}
                <div
                  className="flex items-center gap-2 px-3 py-2.5 bg-muted/40 cursor-pointer hover:bg-muted/60 transition-colors"
                  onClick={() => toggleExpand(portfolio.portfolio_id)}
                >
                  {isExpanded
                    ? <ChevronDown className="size-4 text-muted-foreground shrink-0" />
                    : <ChevronRight className="size-4 text-muted-foreground shrink-0" />
                  }
                  <span className="font-medium text-sm flex-1">{portfolio.name}</span>
                  <span className="text-xs text-muted-foreground mr-2">
                    {portfolioAccounts.length} account{portfolioAccounts.length !== 1 ? "s" : ""}
                  </span>
                  <button
                    className="p-1 text-muted-foreground hover:text-foreground transition-colors"
                    onClick={(e) => { e.stopPropagation(); setRenameTarget({ type: "portfolio", id: portfolio.portfolio_id, name: portfolio.name }); }}
                  >
                    <Pencil className="size-3.5" />
                  </button>
                  <button
                    className="p-1 text-muted-foreground hover:text-destructive transition-colors"
                    onClick={(e) => { e.stopPropagation(); setDeleteTarget({ type: "portfolio", id: portfolio.portfolio_id, name: portfolio.name }); }}
                  >
                    <Trash2 className="size-3.5" />
                  </button>
                </div>

                {/* Accounts list */}
                {isExpanded && (
                  <div className="divide-y">
                    {portfolioAccounts.map((account) => (
                      <div key={account.account_id} className="flex items-center gap-2 px-4 py-2.5 text-sm">
                        <div className="flex-1 min-w-0">
                          <div className="font-medium truncate">{account.name}</div>
                          <div className="text-xs text-muted-foreground">
                            {ACCOUNT_TYPES.find((t) => t.value === account.account_type)?.label ?? account.account_type}
                            {account.broker && ` · ${account.broker}`}
                            {account.account_no && ` · ${account.account_no}`}
                          </div>
                        </div>
                        <button
                          className="p-1 text-muted-foreground hover:text-foreground transition-colors shrink-0"
                          onClick={() => setRenameTarget({ type: "account", id: account.account_id, name: account.name })}
                        >
                          <Pencil className="size-3.5" />
                        </button>
                        <button
                          className="p-1 text-muted-foreground hover:text-destructive transition-colors shrink-0"
                          onClick={() => setDeleteTarget({ type: "account", id: account.account_id, name: account.name })}
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                      </div>
                    ))}

                    {/* Add account button */}
                    <button
                      className="flex items-center gap-2 px-4 py-2.5 text-sm text-muted-foreground hover:text-foreground hover:bg-muted/30 w-full transition-colors"
                      onClick={() => setNewAccount({ portfolioId: portfolio.portfolio_id, name: "", type: "", broker: "", accountNo: "" })}
                    >
                      <Plus className="size-3.5" /> Add account
                    </button>
                  </div>
                )}
              </div>
            );
          })}
        </CardContent>
      </Card>

      {/* New Portfolio Dialog */}
      <Dialog open={showNewPortfolio} onOpenChange={setShowNewPortfolio}>
        <DialogContent>
          <DialogHeader><DialogTitle>New Portfolio</DialogTitle></DialogHeader>
          <div className="space-y-2 py-2">
            <Label>Portfolio name</Label>
            <Input
              placeholder='e.g. "Spouse" or "Kids"'
              value={newPortfolioName}
              onChange={(e) => setNewPortfolioName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreatePortfolio()}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowNewPortfolio(false)}>Cancel</Button>
            <Button onClick={handleCreatePortfolio} disabled={!newPortfolioName.trim()}>Create</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Rename Dialog */}
      <Dialog open={!!renameTarget} onOpenChange={(o) => !o && setRenameTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Rename {renameTarget?.type === "portfolio" ? "Portfolio" : "Account"}</DialogTitle>
          </DialogHeader>
          <div className="space-y-2 py-2">
            <Label>New name</Label>
            <Input
              value={renameTarget?.name ?? ""}
              onChange={(e) => setRenameTarget((t) => t ? { ...t, name: e.target.value } : t)}
              onKeyDown={(e) => e.key === "Enter" && handleRename()}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRenameTarget(null)}>Cancel</Button>
            <Button onClick={handleRename} disabled={!renameTarget?.name?.trim()}>Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* New Account Dialog */}
      <Dialog open={!!newAccount} onOpenChange={(o) => !o && setNewAccount(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>Add Account</DialogTitle></DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1">
              <Label>Account name</Label>
              <Input
                placeholder='e.g. "Zerodha Demat"'
                value={newAccount?.name ?? ""}
                onChange={(e) => setNewAccount((a) => a ? { ...a, name: e.target.value } : a)}
                autoFocus
              />
            </div>
            <div className="space-y-1">
              <Label>Type</Label>
              <Select value={newAccount?.type ?? ""} onValueChange={(v) => setNewAccount((a) => a ? { ...a, type: v, broker: "" } : a)}>
                <SelectTrigger><SelectValue placeholder="Select type" /></SelectTrigger>
                <SelectContent>
                  {ACCOUNT_TYPES.map((t) => (
                    <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {ACCOUNT_TYPES.find((t) => t.value === newAccount?.type)?.brokerRequired && (
              <div className="space-y-1">
                <Label>Broker</Label>
                <Select value={newAccount?.broker ?? ""} onValueChange={(v) => setNewAccount((a) => a ? { ...a, broker: v } : a)}>
                  <SelectTrigger><SelectValue placeholder="Select broker" /></SelectTrigger>
                  <SelectContent>
                    {BROKERS.map((b) => <SelectItem key={b} value={b}>{b}</SelectItem>)}
                  </SelectContent>
                </Select>
              </div>
            )}
            <div className="space-y-1">
              <Label>Account / folio number <span className="text-muted-foreground text-xs">(optional)</span></Label>
              <Input
                placeholder="Account or folio number"
                value={newAccount?.accountNo ?? ""}
                onChange={(e) => setNewAccount((a) => a ? { ...a, accountNo: e.target.value } : a)}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setNewAccount(null)}>Cancel</Button>
            <Button onClick={handleCreateAccount} disabled={!newAccount?.name?.trim() || !newAccount?.type}>Add</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Data & Sync */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Data &amp; Sync</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-sm font-medium">Export data</p>
              <p className="text-xs text-muted-foreground mt-0.5">
                Create an encrypted .ptdata backup to transfer to another device.
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => setShowExport(true)} className="shrink-0">
              <Upload className="size-4 mr-1.5" /> Export
            </Button>
          </div>
          <div className="border-t" />
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-sm font-medium">Import data</p>
              <p className="text-xs text-muted-foreground mt-0.5">
                Restore from a .ptdata file. <span className="text-destructive font-medium">Wipes all current data.</span>
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => setShowImport(true)} className="shrink-0">
              <Download className="size-4 mr-1.5" /> Import
            </Button>
          </div>
        </CardContent>
      </Card>

      <ExportDialog open={showExport} onOpenChange={setShowExport} />
      <ImportDialog open={showImport} onOpenChange={setShowImport} onImported={() => window.location.reload()} />

      {/* Delete Confirmation */}
      <AlertDialog open={!!deleteTarget} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {deleteTarget?.type === "portfolio" ? "Portfolio" : "Account"}?</AlertDialogTitle>
            <AlertDialogDescription>
              "{deleteTarget?.name}" will be permanently deleted. This cannot be undone.
              {deleteTarget?.type === "portfolio" && " All accounts must be removed first."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
