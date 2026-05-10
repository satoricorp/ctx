"use client";

import { GeistMono } from "geist/font/mono";
import { Check, Copy } from "lucide-react";
import { useCallback, useState } from "react";

const INSTALL_CMD = "cargo install --git https://github.com/satoricorp/ctx --bins";

export function InstallCommand() {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(INSTALL_CMD);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }, []);

  return (
    <div
      className={`${GeistMono.className} bg-muted text-foreground flex w-full max-w-2xl items-center gap-3 rounded-xl border px-4 py-3`}
      style={{ borderColor: "color-mix(in oklab, var(--accent-highlight) 24%, var(--border))" }}
    >
      <code className="min-w-0 flex-1 overflow-x-auto text-xs md:text-sm">
        {INSTALL_CMD}
      </code>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? "Copied" : "Copy install command"}
        className="bg-background text-foreground hover:bg-muted shrink-0 rounded-lg border p-2 transition"
        style={{
          borderColor: "color-mix(in oklab, var(--accent-highlight) 40%, var(--border))",
          color: copied ? "var(--accent-highlight)" : undefined,
        }}
      >
        {copied ? (
          <Check className="size-4" aria-hidden />
        ) : (
          <Copy className="size-4" aria-hidden />
        )}
      </button>
    </div>
  );
}
