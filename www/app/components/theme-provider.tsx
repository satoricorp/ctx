"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";

export type Theme = "light" | "dark";

type ThemeContextValue = {
  resolvedTheme: Theme;
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

const STORAGE_KEY = "theme";

function readStoredTheme(): Theme | null {
  if (typeof window === "undefined") return null;
  const v = localStorage.getItem(STORAGE_KEY);
  if (v === "light" || v === "dark") return v;
  return null;
}

function systemTheme(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function applyDom(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [resolvedTheme, setResolvedTheme] = useState<Theme>("light");

  useEffect(() => {
    const next = readStoredTheme() ?? systemTheme();
    setResolvedTheme(next);
    applyDom(next);
  }, []);

  const setTheme = useCallback((theme: Theme) => {
    localStorage.setItem(STORAGE_KEY, theme);
    setResolvedTheme(theme);
    applyDom(theme);
  }, []);

  const toggleTheme = useCallback(() => {
    setTheme(resolvedTheme === "dark" ? "light" : "dark");
  }, [resolvedTheme, setTheme]);

  const value = useMemo(
    () => ({ resolvedTheme, setTheme, toggleTheme }),
    [resolvedTheme, setTheme, toggleTheme],
  );

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useAppTheme() {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error("useAppTheme must be used within ThemeProvider");
  }
  return ctx;
}
