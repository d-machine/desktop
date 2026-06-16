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
import { ExportDialog } from "@/components/sync/ExportDialog";
import { ImportDialog } from "@/components/sync/ImportDialog";

interface Person    { person_id: number; name: string; pan?: string; }
interface Portfolio { portfolio_id: number; name: string; person_id?: number; }
interface Account {
  account_id: number; portfolio_id: number; name: string;
  account_type: string; broker?: string; account_no?: string;
}

function maskPan(pan: string): string {
  if (pan.length < 5) return pan;
  return pan.slice(0, 5) + "·····";
}

export function SettingsPage() {
  const [persons,    setPersons]    = useState<Person[]>([]);
  const [portfolios, setPortfolios] = useState<Portfolio[]>([]);
  const [accounts,   setAccounts]   = useState<Account[]>([]);
  const [expanded,   setExpanded]   = useState<Set<number>>(new Set());
  const [error,      setError]      = useState("");
  const [showExport, setShowExport] = useState(false);
  const [showImport, setShowImport] = useState(false);

  const [serverUrl,      setServerUrl]      = useState("");
  const [serverUrlSaved, setServerUrlSaved] = useState(false);

  // Person dialog state
  const [showNewPerson,   setShowNewPerson]   = useState(false);
  const [newPersonName,   setNewPersonName]   = useState("");
  const [newPersonPan,    setNewPersonPan]    = useState("");
  const [editPerson,      setEditPerson]      = useState<Person | null>(null);

  // Portfolio dialog state
  const [newPortfolioName,     setNewPortfolioName]     = useState("");
  const [newPortfolioPersonId, setNewPortfolioPersonId] = useState<string>("");
  const [showNewPortfolio,     setShowNewPortfolio]     = useState(false);

  // Shared rename/delete/add-account dialog state
  const [renameTarget, setRenameTarget] = useState<{ type: "portfolio" | "account"; id: number; name: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ type: "portfolio" | "account"; id: number; name: string } | null>(null);
  const [newAccount,   setNewAccount]   = useState<{ portfolioId: number; name: string; type: string; broker: string; accountNo: string } | null>(null);

  const load = async () => {
    const [pe, p, a] = await Promise.all([
      invoke<Person[]>("get_persons"),
      invoke<Portfolio[]>("get_portfolios"),
      invoke<Account[]>("get_accounts", { portfolioId: null }),
    ]);
    setPersons(pe);
    setPortfolios(p);
    setAccounts(a);
    setExpanded(new Set(p.map((x) => x.portfolio_id)));
  };

  useEffect(() => {
    load();
    invoke<string | null>("get_setting", { key: "server_url" }).then((v) => {
      if (v) setServerUrl(v);
    });
  }, []);

  const handleSaveServerUrl = async () => {
    try {
      await invoke("set_setting", { key: "server_url", value: serverUrl.trim() });
      setServerUrlSaved(true);
      setTimeout(() => setServerUrlSaved(false), 2000);
    } catch (e: any) { setError(e.toString()); }
  };

  const toggleExpand = (id: number) =>
    setExpanded((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });

  // ── Person CRUD ──────────────────────────────────────────────────────────

  const handleCreatePerson = async () => {
    if (!newPersonName.trim()) return;
    try {
      await invoke("create_person", { input: { name: newPersonName.trim(), pan: newPersonPan.trim() || null } });
      setNewPersonName("");
      setNewPersonPan("");
      setShowNewPerson(false);
      await load();
    } catch (e: any) { setError(e.toString()); }
  };

  const handleUpdatePerson = async () => {
    if (!editPerson) return;
    try {
      await invoke("update_person", {
        personId: editPerson.person_id,
        input: { name: editPerson.name.trim() || null, pan: editPerson.pan?.trim() || null },
      });
      setEditPerson(null);
      await load();
    } catch (e: any) { setError(e.toString()); }
  };

  // ── Portfolio CRUD ───────────────────────────────────────────────────────

  const handleCreatePortfolio = async () => {
    if (!newPortfolioName.trim()) return;
    try {
      await invoke("create_portfolio", {
        input: {
          name: newPortfolioName.trim(),
          person_id: newPortfolioPersonId ? parseInt(newPortfolioPersonId) : null,
        },
      });
      setNewPortfolioName("");
      setNewPortfolioPersonId("");
      setShowNewPortfolio(false);
      await load();
    } catch (e: any) { setError(e.toString()); }
  };

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
          <p className="text-muted-foreground text-sm">Manage people, portfolios, accounts and preferences.</p>
        </div>
      </div>

      {error && (
        <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
          {error}
          <button className="ml-2 underline text-xs" onClick={() => setError("")}>dismiss</button>
        </div>
      )}

      {/* People */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-3">
          <CardTitle className="text-base">People</CardTitle>
          <Button size="sm" variant="outline" onClick={() => setShowNewPerson(true)}>
            <Plus className="size-4 mr-1" /> Add Person
          </Button>
        </CardHeader>
        <CardContent className="space-y-1">
          {persons.length === 0 && (
            <p className="text-sm text-muted-foreground text-center py-4">No people yet.</p>
          )}
          {persons.map((person) => {
            const personPortfolios = portfolios.filter(p => p.person_id === person.person_id);
            return (
              <div key={person.person_id} className="flex items-center gap-2 rounded-md px-2 py-2 hover:bg-muted/40 transition-colors">
                <div className="flex-1 min-w-0">
                  <span className="font-medium text-sm">{person.name}</span>
                  <span className="text-xs text-muted-foreground ml-2">
                    {person.pan ? maskPan(person.pan) : "no PAN"}
                  </span>
                  {personPortfolios.length > 0 && (
                    <span className="text-xs text-muted-foreground ml-2">
                      · {personPortfolios.length} portfolio{personPortfolios.length !== 1 ? "s" : ""}
                    </span>
                  )}
                </div>
                <button
                  className="p-1 text-muted-foreground hover:text-foreground transition-colors"
                  onClick={() => setEditPerson({ ...person })}
                >
                  <Pencil className="size-3.5" />
                </button>
              </div>
            );
          })}
        </CardContent>
      </Card>

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
            const owner = portfolio.person_id ? persons.find(p => p.person_id === portfolio.person_id) : null;
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
                  {owner && (
                    <span className="text-xs text-muted-foreground mr-1">{owner.name}</span>
                  )}
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

      {/* Add Person Dialog */}
      <Dialog open={showNewPerson} onOpenChange={(o) => { if (!o) { setShowNewPerson(false); setNewPersonName(""); setNewPersonPan(""); } }}>
        <DialogContent>
          <DialogHeader><DialogTitle>Add Person</DialogTitle></DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1">
              <Label>Name</Label>
              <Input
                placeholder='e.g. "Rahul Sharma"'
                value={newPersonName}
                onChange={(e) => setNewPersonName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleCreatePerson()}
                autoFocus
              />
            </div>
            <div className="space-y-1">
              <Label>PAN <span className="text-muted-foreground text-xs">(optional)</span></Label>
              <Input
                placeholder="e.g. ABCDE1234F"
                value={newPersonPan}
                onChange={(e) => setNewPersonPan(e.target.value.toUpperCase())}
                className="uppercase tracking-widest"
                maxLength={10}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => { setShowNewPerson(false); setNewPersonName(""); setNewPersonPan(""); }}>Cancel</Button>
            <Button onClick={handleCreatePerson} disabled={!newPersonName.trim()}>Add</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Person Dialog */}
      <Dialog open={!!editPerson} onOpenChange={(o) => !o && setEditPerson(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>Edit Person</DialogTitle></DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1">
              <Label>Name</Label>
              <Input
                value={editPerson?.name ?? ""}
                onChange={(e) => setEditPerson((p) => p ? { ...p, name: e.target.value } : p)}
                onKeyDown={(e) => e.key === "Enter" && handleUpdatePerson()}
                autoFocus
              />
            </div>
            <div className="space-y-1">
              <Label>PAN <span className="text-muted-foreground text-xs">(optional — clear to remove)</span></Label>
              <Input
                placeholder="e.g. ABCDE1234F"
                value={editPerson?.pan ?? ""}
                onChange={(e) => setEditPerson((p) => p ? { ...p, pan: e.target.value.toUpperCase() } : p)}
                className="uppercase tracking-widest"
                maxLength={10}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditPerson(null)}>Cancel</Button>
            <Button onClick={handleUpdatePerson} disabled={!editPerson?.name?.trim()}>Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* New Portfolio Dialog */}
      <Dialog open={showNewPortfolio} onOpenChange={(o) => { if (!o) { setShowNewPortfolio(false); setNewPortfolioName(""); setNewPortfolioPersonId(""); } }}>
        <DialogContent>
          <DialogHeader><DialogTitle>New Portfolio</DialogTitle></DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1">
              <Label>Portfolio name</Label>
              <Input
                placeholder='e.g. "Spouse" or "Kids"'
                value={newPortfolioName}
                onChange={(e) => setNewPortfolioName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleCreatePortfolio()}
                autoFocus
              />
            </div>
            {persons.length > 0 && (
              <div className="space-y-1">
                <Label>Owner <span className="text-muted-foreground text-xs">(optional)</span></Label>
                <Select value={newPortfolioPersonId} onValueChange={(v) => setNewPortfolioPersonId(v ?? "")}>
                  <SelectTrigger><SelectValue placeholder="Select person" /></SelectTrigger>
                  <SelectContent>
                    {persons.map((p) => (
                      <SelectItem key={p.person_id} value={p.person_id.toString()}>{p.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => { setShowNewPortfolio(false); setNewPortfolioName(""); setNewPortfolioPersonId(""); }}>Cancel</Button>
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
              <Select value={newAccount?.type ?? ""} onValueChange={(v) => setNewAccount((a) => a ? { ...a, type: v ?? "", broker: "" } : null)}>
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
                <Select value={newAccount?.broker ?? ""} onValueChange={(v) => setNewAccount((a) => a ? { ...a, broker: v ?? "" } : null)}>
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

      {/* Server URL */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Server</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-2">
            <Label htmlFor="server-url">API server URL</Label>
            <div className="flex gap-2">
              <Input
                id="server-url"
                value={serverUrl}
                onChange={(e) => { setServerUrl(e.target.value); setServerUrlSaved(false); }}
                onKeyDown={(e) => e.key === "Enter" && handleSaveServerUrl()}
                placeholder="https://arthdeskapi.ashokitservices.com"
                className="font-mono text-sm"
              />
              <Button variant="outline" size="sm" onClick={handleSaveServerUrl} className="shrink-0">
                {serverUrlSaved ? "Saved" : "Save"}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">Used for price sync and instrument resolution.</p>
          </div>
        </CardContent>
      </Card>

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
