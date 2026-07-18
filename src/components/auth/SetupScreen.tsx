import { useState } from "react";
import { apiGet, apiPost, setSessionToken } from "@/lib/api";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PinInput } from "./PinInput";

type Step = "server-login" | "set-pin" | "confirm-pin" | "set-passphrase" | "save-recovery";

interface ValidateResult {
  email: string;
  access_token: string;
  refresh_token: string;
  subscription_status: string;
  subscription_expires_at: string;
}

interface SetupScreenProps {
  onComplete: () => void;
}

export function SetupScreen({ onComplete }: SetupScreenProps) {
  const [step, setStep] = useState<Step>("server-login");
  const [serverEmail, setServerEmail] = useState("");
  const [serverPassword, setServerPassword] = useState("");
  const [validatedCreds, setValidatedCreds] = useState<ValidateResult | null>(null);
  const [pin, setPin] = useState("");
  const [confirmedPin, setConfirmedPin] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirmPassphrase, setConfirmPassphrase] = useState("");
  const [recoveryJson, setRecoveryJson] = useState("");
  const [recoverySaved, setRecoverySaved] = useState(false);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const handleServerLogin = async () => {
    if (!serverEmail || !serverPassword) { setError("Enter your email and password"); return; }
    setError("");
    setLoading(true);
    try {
      // Validate against remote server without requiring a local session
      const result = await apiPost<ValidateResult>("/server-auth/validate", { email: serverEmail, password: serverPassword });
      setValidatedCreds(result);
      setStep("set-pin");
    } catch (e: any) {
      setError(e?.message || "Login failed. Check your credentials.");
    } finally {
      setLoading(false);
    }
  };

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
      const res = await apiPost<{ session_token: string; recovery_json: string }>("/auth/setup", { pin, passphrase });
      setSessionToken(res.session_token);
      setRecoveryJson(res.recovery_json);
      // Now that the DB exists and session is active, store server tokens and sync persons
      if (validatedCreds) {
        await apiPost("/server-auth/login", { email: validatedCreds.email, password: serverPassword });
        await apiGet("/server-auth/persons").catch(() => {});
      }
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
        {step === "server-login" && (
          <>
            <CardHeader className="text-center">
              <CardTitle className="text-2xl">Welcome back</CardTitle>
              <CardDescription>
                Sign in to continue to ArthaDesk.{" "}
                <a
                  href="https://arthdesk.ashokitservices.com/auth#register"
                  target="_blank"
                  rel="noreferrer"
                  className="underline"
                >
                  Don't have an account?
                </a>
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label>Email</Label>
                <Input
                  type="email"
                  placeholder="you@example.com"
                  value={serverEmail}
                  onChange={(e) => setServerEmail(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleServerLogin()}
                  autoFocus
                />
              </div>
              <div className="space-y-2">
                <Label>Password</Label>
                <Input
                  type="password"
                  placeholder="Your password"
                  value={serverPassword}
                  onChange={(e) => setServerPassword(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleServerLogin()}
                />
              </div>
              {error && <p className="text-destructive text-sm">{error}</p>}
              <Button className="w-full" onClick={handleServerLogin} disabled={loading}>
                {loading ? "Signing in…" : "Sign in"}
              </Button>
            </CardContent>
          </>
        )}

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
