import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Eye, EyeOff, FolderOpen, CheckCircle2 } from "lucide-react";
import { cn } from "@/lib/utils";

interface ExportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

type Step = "form" | "exporting" | "done" | "error";

export function ExportDialog({ open, onOpenChange }: ExportDialogProps) {
  const [pin, setPin] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [destPath, setDestPath] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [step, setStep] = useState<Step>("form");
  const [error, setError] = useState("");

  const reset = () => {
    setPin(""); setPassword(""); setConfirmPassword("");
    setDestPath(""); setShowPin(false); setShowPassword(false);
    setStep("form"); setError("");
  };

  const handlePickFolder = async () => {
    const folder = await openDialog({ directory: true, title: "Choose export location" });
    if (folder) {
      const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 16);
      setDestPath(`${folder}/portfolio-backup-${ts}.ptdata`);
    }
  };

  const canExport = pin && password && password === confirmPassword && destPath;

  const handleExport = async () => {
    setStep("exporting");
    setError("");
    try {
      await invoke("export_data", { pin, password, destPath });
      setStep("done");
    } catch (e: any) {
      setError(e.toString());
      setStep("error");
    }
  };

  const handleClose = () => { reset(); onOpenChange(false); };

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) handleClose(); }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Export Data</DialogTitle>
          <DialogDescription>
            Create an encrypted backup of your portfolio data to transfer to another device.
          </DialogDescription>
        </DialogHeader>

        {step === "done" ? (
          <div className="py-6 flex flex-col items-center gap-3 text-center">
            <CheckCircle2 className="size-12 text-green-500" />
            <p className="font-medium">Export complete</p>
            <p className="text-sm text-muted-foreground break-all">{destPath}</p>
            <p className="text-xs text-muted-foreground mt-1">
              Keep this file and your export password safe — you need both to import on another device.
            </p>
          </div>
        ) : (
          <div className="space-y-4 py-1">
            {/* PIN */}
            <div className="space-y-1.5">
              <Label>Your current PIN</Label>
              <div className="relative">
                <Input
                  type={showPin ? "text" : "password"}
                  placeholder="Enter PIN"
                  value={pin}
                  onChange={(e) => setPin(e.target.value)}
                  disabled={step === "exporting"}
                />
                <button
                  type="button"
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                  onClick={() => setShowPin((v) => !v)}
                  tabIndex={-1}
                >
                  {showPin ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                </button>
              </div>
              <p className="text-xs text-muted-foreground">Used to unlock your database for export.</p>
            </div>

            {/* Export password */}
            <div className="space-y-1.5">
              <Label>Export password</Label>
              <div className="relative">
                <Input
                  type={showPassword ? "text" : "password"}
                  placeholder="Choose a password for this backup"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  disabled={step === "exporting"}
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
              <Input
                type={showPassword ? "text" : "password"}
                placeholder="Confirm export password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                disabled={step === "exporting"}
                className={cn(confirmPassword && password !== confirmPassword && "border-destructive")}
              />
              {confirmPassword && password !== confirmPassword && (
                <p className="text-xs text-destructive">Passwords do not match</p>
              )}
              <p className="text-xs text-muted-foreground">
                This password protects the export file. You'll need it when importing on another device.
              </p>
            </div>

            {/* Destination */}
            <div className="space-y-1.5">
              <Label>Save location</Label>
              <div className="flex gap-2">
                <div className="flex-1 border rounded-md px-3 py-2 text-sm text-muted-foreground truncate bg-muted/30 font-mono text-xs">
                  {destPath ? destPath.split("/").slice(-1)[0] : "No location selected"}
                </div>
                <Button variant="outline" onClick={handlePickFolder} disabled={step === "exporting"}>
                  <FolderOpen className="size-4 mr-1" /> Browse
                </Button>
              </div>
              {destPath && (
                <p className="text-xs text-muted-foreground break-all">{destPath}</p>
              )}
            </div>

            {step === "error" && (
              <div className="text-sm text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
                {error}
              </div>
            )}
          </div>
        )}

        <DialogFooter>
          {step === "done" ? (
            <Button onClick={handleClose}>Done</Button>
          ) : (
            <>
              <Button variant="outline" onClick={handleClose} disabled={step === "exporting"}>Cancel</Button>
              <Button
                onClick={handleExport}
                disabled={!canExport || step === "exporting"}
              >
                {step === "exporting" ? "Exporting…" : "Export"}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
