"use client";

import { Moon, Sun } from "lucide-react";
import { useCallback, useSyncExternalStore } from "react";

function subscribe(onStoreChange: () => void) {
  const observer = new MutationObserver(onStoreChange);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class"],
  });
  return () => observer.disconnect();
}

function getDarkSnapshot() {
  return document.documentElement.classList.contains("dark");
}

function getServerDarkSnapshot() {
  return false;
}

export function ThemeToggle() {
  const dark = useSyncExternalStore(subscribe, getDarkSnapshot, getServerDarkSnapshot);

  const toggle = useCallback(() => {
    const next = !document.documentElement.classList.contains("dark");
    document.documentElement.classList.toggle("dark", next);
    localStorage.setItem("theme", next ? "dark" : "light");
  }, []);

  return (
    <button
      type="button"
      onClick={toggle}
      className="text-muted-foreground hover:text-foreground focus-visible:ring-ring inline-flex size-9 shrink-0 items-center justify-center self-center rounded-md transition-colors focus-visible:ring-2 focus-visible:outline-none"
      aria-label={dark ? "Switch to light mode" : "Switch to dark mode"}
      aria-pressed={dark}
    >
      {dark ? <Sun className="size-4" aria-hidden /> : <Moon className="size-4" aria-hidden />}
    </button>
  );
}
