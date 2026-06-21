import { useState } from "react";
import { apiPost } from "@/lib/api";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Eye, EyeOff, FolderOpen, CheckCircle2, AlertTriangle } from "lucide-react";

interface ImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onImported: () => void;
}

type Step = "form" | "confirm" | "importing" | "done" | "error";

export function ImportDialog({ open, onOpenChange, onImported }: ImportDialogProps) {
  const [srcPath, setSrcPath] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [understood, setUnderstood] = useState(false);
  const [step, setStep] = useState<Step>("form");
  const [error, setError] = useState("");

  const reset = () => {
    setSrcPath(""); setPassword(""); setShowPassword(false);
    setUnderstood(false); setStep("form"); setError("");
  };

  const handlePickFile = async () => {
    const file = await openDialog({
      title: "Select export file",
      filters: [{ name: "Portfolio Tracker Backup", extensions: ["ptdata"] }],
    });
    if (file) setSrcPath(file as string);
  };

  const handleClose = () => { reset(); onOpenChange(false); };

  const handleProceedToConfirm = () => {
    if (!srcPath || !password) return;
    setStep("confirm");
  };

  const handleImport = async () => {
    setStep("importing");
    setError("");
    try {
      await apiPost("/backup/import", { password, src_path: srcPath });
      setStep("done");
    } catch (e: any) {
      setError(e.toString());
      setStep("error");
    }
  };

  const handleDone = () => {
    onImported();
    handleClose();
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) handleClose(); }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Import Data</DialogTitle>
          <DialogDescription>
            Restore portfolio data from a .ptdata export file.
          </DialogDescription>
        </DialogHeader>

        {/* Step: form */}
        {(step === "form" || step === "error") && (
          <div className="space-y-4 py-1">
            {/* File picker */}
            <div className="space-y-1.5">
              <Label>Backup file (.ptdata)</Label>
              <div className="flex gap-2">
                <div className="flex-1 border rounded-md px-3 py-2 text-sm text-muted-foreground truncate bg-muted/30 font-mono text-xs">
                  {srcPath ? srcPath.split("/").slice(-1)[0] : "No file selected"}
                </div>
                <Button variant="outline" onClick={handlePickFile}>
                  <FolderOpen className="size-4 mr-1" /> Browse
                </Button>
              </div>
            </div>

            {/* Export password */}
            <div className="space-y-1.5">
              <Label>Export password</Label>
              <div className="relative">
                <Input
                  type={showPassword ? "text" : "password"}
                  placeholder="Password used when exporting"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                />
                <button
                  type="button"
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                  onClick={() => setShowPassword((v) => !v)}
                  tabIndex={-1}
                >
                  {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                </button>
              </div>
            </div>

            {step === "error" && (
              <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
                {error}
              </div>
            )}
          </div>
        )}

        {/* Step: confirm — hard confirmation */}
        {step === "confirm" && (
          <div className="space-y-4 py-1">
            <div className="flex gap-3 p-3 bg-destructive/10 border border-destructive/20 rounded-md">
              <AlertTriangle className="size-5 text-destructive shrink-0 mt-0.5" />
              <div className="text-sm">
                <p className="font-semibold text-destructive">This will permanently wipe all current data.</p>
                <p className="text-muted-foreground mt-1">
                  All portfolios, accounts, transactions and holdings on this device will be deleted and replaced with the contents of the backup file. This cannot be undone.
                </p>
              </div>
            </div>

            <div className="flex items-start gap-2.5">
              <input
                id="understood"
                type="checkbox"
                checked={understood}
                onChange={(e) => setUnderstood(e.target.checked)}
                className="mt-0.5 h-4 w-4 rounded border-border accent-primary cursor-pointer shrink-0"
              />
              <label htmlFor="understood" className="text-sm cursor-pointer leading-snug">
                I understand this will permanently delete all current data on this device and replace it with the imported data.
              </label>
            </div>
          </div>
        )}

        {/* Step: importing */}
        {step === "importing" && (
          <div className="py-6 text-center text-sm text-muted-foreground">
            Importing… please wait.
          </div>
        )}

        {/* Step: done */}
        {step === "done" && (
          <div className="py-6 flex flex-col items-center gap-3 text-center">
            <CheckCircle2 className="size-12 text-green-500" />
            <p className="font-medium">Import complete</p>
            <p className="text-sm text-muted-foreground">Your data has been restored successfully.</p>
          </div>
        )}

        <DialogFooter>
          {step === "form" && (
            <>
              <Button variant="outline" onClick={handleClose}>Cancel</Button>
              <Button onClick={handleProceedToConfirm} disabled={!srcPath || !password}>
                Continue
              </Button>
            </>
          )}
          {step === "confirm" && (
            <>
              <Button variant="outline" onClick={() => setStep("form")}>Back</Button>
              <Button
                variant="destructive"
                onClick={handleImport}
                disabled={!understood}
              >
                Wipe & Import
              </Button>
            </>
          )}
          {step === "error" && (
            <>
              <Button variant="outline" onClick={handleClose}>Cancel</Button>
              <Button onClick={handleProceedToConfirm} disabled={!srcPath || !password}>
                Try Again
              </Button>
            </>
          )}
          {step === "done" && (
            <Button onClick={handleDone}>Done</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
