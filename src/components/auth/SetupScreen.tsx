import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PinInput } from "./PinInput";

type Step = "set-pin" | "confirm-pin" | "set-passphrase" | "save-recovery";

interface SetupScreenProps {
  onComplete: () => void;
}

export function SetupScreen({ onComplete }: SetupScreenProps) {
  const [step, setStep] = useState<Step>("set-pin");
  const [pin, setPin] = useState("");
  const [confirmedPin, setConfirmedPin] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirmPassphrase, setConfirmPassphrase] = useState("");
  const [recoveryJson, setRecoveryJson] = useState("");
  const [recoverySaved, setRecoverySaved] = useState(false);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const handlePinSet = () => {
    if (pin.length < 6) { setError("PIN must be 6 digits"); return; }
    setError("");
    setStep("confirm-pin");
  };

  const handlePinConfirm = () => {
    if (confirmedPin !== pin) { setError("PINs do not match"); return; }
    setError("");
    setStep("set-passphrase");
  };

  const handlePassphraseSet = async () => {
    if (passphrase.length < 8) { setError("Passphrase must be at least 8 characters"); return; }
    if (passphrase !== confirmPassphrase) { setError("Passphrases do not match"); return; }
    setError("");
    setLoading(true);
    try {
      const recovery = await invoke<string>("setup", { pin, passphrase });
      setRecoveryJson(recovery);
      setStep("save-recovery");
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const handleSaveRecovery = async () => {
    try {
      const filePath = await save({
        title: "Save Recovery File",
        defaultPath: "portfolio-tracker-recovery.ptbak",
        filters: [{ name: "Recovery File", extensions: ["ptbak"] }],
      });
      if (filePath) {
        await writeTextFile(filePath, recoveryJson);
        setRecoverySaved(true);
      }
    } catch (e: any) {
      setError(e.toString());
    }
  };

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-4">
      <Card className="w-full max-w-md">
        {step === "set-pin" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Set your PIN</CardTitle>
              <CardDescription>
                You'll enter this every time you open the app.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <PinInput onChange={setPin} error={!!error} />
              {error && <p className="text-destructive text-sm text-center">{error}</p>}
              <Button className="w-full" onClick={handlePinSet} disabled={pin.length < 6}>
                Continue
              </Button>
            </CardContent>
          </>
        )}

        {step === "confirm-pin" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Confirm your PIN</CardTitle>
              <CardDescription>Enter your PIN again to confirm.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <PinInput onChange={setConfirmedPin} error={!!error} />
              {error && <p className="text-destructive text-sm text-center">{error}</p>}
              <div className="flex gap-3">
                <Button variant="outline" className="flex-1" onClick={() => { setStep("set-pin"); setError(""); }}>
                  Back
                </Button>
                <Button className="flex-1" onClick={handlePinConfirm} disabled={confirmedPin.length < 6}>
                  Continue
                </Button>
              </div>
            </CardContent>
          </>
        )}

        {step === "set-passphrase" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Set a recovery passphrase</CardTitle>
              <CardDescription>
                If you forget your PIN, this passphrase is your only way back in.
                Write it down and keep it safe.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label>Recovery passphrase</Label>
                <Input
                  type="password"
                  placeholder="At least 8 characters"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label>Confirm passphrase</Label>
                <Input
                  type="password"
                  placeholder="Repeat your passphrase"
                  value={confirmPassphrase}
                  onChange={(e) => setConfirmPassphrase(e.target.value)}
                />
              </div>
              {error && <p className="text-destructive text-sm">{error}</p>}
              <div className="flex gap-3">
                <Button variant="outline" className="flex-1" onClick={() => { setStep("confirm-pin"); setError(""); }}>
                  Back
                </Button>
                <Button className="flex-1" onClick={handlePassphraseSet} disabled={loading}>
                  {loading ? "Setting up…" : "Continue"}
                </Button>
              </div>
            </CardContent>
          </>
        )}

        {step === "save-recovery" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Save your recovery file</CardTitle>
              <CardDescription>
                This file contains your encrypted recovery key. Save it somewhere safe —
                USB drive, email to yourself, or cloud storage.
                <br /><br />
                <strong>Without this file and your passphrase, a forgotten PIN means permanent data loss.</strong>
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {error && <p className="text-destructive text-sm">{error}</p>}
              <Button
                variant="outline"
                className="w-full"
                onClick={handleSaveRecovery}
              >
                {recoverySaved ? "Saved ✓ — Save again to another location" : "Save recovery file (.ptbak)"}
              </Button>
              <Button
                className="w-full disabled:opacity-40"
                disabled={!recoverySaved}
                onClick={onComplete}
              >
                I have saved my recovery file — Open App
              </Button>
              {!recoverySaved && (
                <p className="text-muted-foreground text-xs text-center">
                  You must save the recovery file before continuing.
                </p>
              )}
            </CardContent>
          </>
        )}
      </Card>
    </div>
  );
}
