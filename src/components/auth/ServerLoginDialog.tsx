import { useState } from "react";
import { apiGet, apiPost } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

export interface ServerAuthState {
  logged_in: boolean;
  email: string;
  subscription_status: string;
  subscription_expires_at: string;
}

interface ServerLoginDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: (state: ServerAuthState) => void;
}

export function ServerLoginDialog({ open: isOpen, onOpenChange, onSuccess }: ServerLoginDialogProps) {
  const [view, setView]             = useState<"login" | "forgot">("login");
  const [email, setEmail]           = useState("");
  const [password, setPassword]     = useState("");
  const [forgotEmail, setForgotEmail] = useState("");
  const [error, setError]           = useState("");
  const [forgotMsg, setForgotMsg]   = useState("");
  const [loading, setLoading]       = useState(false);

  const WEBSITE_URL = "https://arthdeskapi.ashokitservices.com";

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      await apiPost("/server-auth/login", { email, password });
      const status = await apiGet<ServerAuthState>("/server-auth/status");
      onSuccess(status);
      onOpenChange(false);
      setEmail(""); setPassword("");
    } catch (err: unknown) {
      setError(String(err).replace(/^Error:\s*/, ""));
    } finally {
      setLoading(false);
    }
  };

  const handleForgot = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setForgotMsg("");
    try {
      await apiPost("/server-auth/forgot-password", { email: forgotEmail });
      setForgotMsg("If that email is registered, a reset link has been sent. Check your inbox.");
    } catch {
      setForgotMsg("If that email is registered, a reset link has been sent. Check your inbox.");
    } finally {
      setLoading(false);
    }
  };

  const openWebsite = (path: string) => openUrl(`${WEBSITE_URL}/${path}`).catch(() => {});

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>
            {view === "login" ? "Login to server" : "Reset password"}
          </DialogTitle>
        </DialogHeader>

        {view === "login" && (
          <form onSubmit={handleLogin} className="space-y-4">
            <div className="space-y-1.5">
              <Label htmlFor="srv-email">Email</Label>
              <Input
                id="srv-email"
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
                autoComplete="email"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="srv-password">Password</Label>
              <Input
                id="srv-password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                autoComplete="current-password"
              />
            </div>

            {error && <p className="text-destructive text-sm">{error}</p>}

            <Button type="submit" className="w-full" disabled={loading}>
              {loading ? "Logging in…" : "Login"}
            </Button>

            <div className="flex flex-col gap-1 text-center text-sm text-muted-foreground">
              <button
                type="button"
                onClick={() => { setView("forgot"); setError(""); setForgotMsg(""); }}
                className="hover:text-foreground transition-colors"
              >
                Forgot password?
              </button>
              <button
                type="button"
                onClick={() => openWebsite("auth.html#register")}
                className="hover:text-foreground transition-colors"
              >
                No account? Register on website
              </button>
            </div>
          </form>
        )}

        {view === "forgot" && (
          <form onSubmit={handleForgot} className="space-y-4">
            <p className="text-sm text-muted-foreground">
              Enter your email and we'll send a reset link. The link opens in your browser.
            </p>
            <div className="space-y-1.5">
              <Label htmlFor="forgot-email">Email</Label>
              <Input
                id="forgot-email"
                type="email"
                value={forgotEmail}
                onChange={(e) => setForgotEmail(e.target.value)}
                required
              />
            </div>

            {forgotMsg && <p className="text-sm text-muted-foreground">{forgotMsg}</p>}

            <Button type="submit" className="w-full" disabled={loading}>
              {loading ? "Sending…" : "Send reset link"}
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="w-full"
              onClick={() => setView("login")}
            >
              ← Back to login
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
