import { GeistMono } from "geist/font/mono";
import { InstallCommand } from "./install-command";
import { Logo } from "./logo";

export default function Home() {
  return (
    <div className="flex min-h-[100vh] flex-col items-center justify-center gap-6 px-4">
      <div className="flex flex-col items-center gap-2">
        <Logo />
        <p
          className={`${GeistMono.className} text-xl text-slate-800 dark:text-slate-200`}
        >
          Docker for context.
        </p>
      </div>
      <InstallCommand />
    </div>
  );
}
