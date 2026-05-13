"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import {
  NavigationMenu,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
} from "@/components/ui/navigation-menu";
import { berkeleyMono, blobLogo } from "./fonts";
import { ThemeToggle } from "./theme-toggle";

const GITHUB_URL = "https://github.com/satoricorp/ctx";
const DISCORD_URL = "https://discord.gg/fB2VPs5zx";

function GitHubIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      fill="currentColor"
      aria-hidden
    >
      <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.09 3.29 9.39 7.86 10.91.58.1.79-.25.79-.56v-2.15c-3.2.7-3.87-1.36-3.87-1.36-.52-1.33-1.28-1.68-1.28-1.68-1.05-.72.08-.7.08-.7 1.15.08 1.76 1.19 1.76 1.19 1.03 1.75 2.69 1.24 3.34.95.1-.74.4-1.24.73-1.53-2.55-.29-5.23-1.28-5.23-5.68 0-1.25.45-2.28 1.18-3.08-.12-.29-.51-1.46.11-3.04 0 0 .96-.31 3.16 1.18a10.86 10.86 0 0 1 5.75 0c2.19-1.49 3.15-1.18 3.15-1.18.63 1.58.24 2.75.12 3.04.74.8 1.18 1.83 1.18 3.08 0 4.42-2.69 5.38-5.25 5.67.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.51 11.51 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z" />
    </svg>
  );
}

function DiscordIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      fill="currentColor"
      aria-hidden
    >
      <path d="M20.32 4.37a19.8 19.8 0 0 0-4.93-1.52.07.07 0 0 0-.07.03c-.21.38-.45.88-.62 1.27a18.27 18.27 0 0 0-5.49 0 12.6 12.6 0 0 0-.63-1.27.07.07 0 0 0-.07-.03A19.74 19.74 0 0 0 3.58 4.37a.06.06 0 0 0-.03.02C.43 9.05-.42 13.58 0 18.06a.08.08 0 0 0 .03.05 19.9 19.9 0 0 0 6.06 3.06.07.07 0 0 0 .08-.03c.47-.64.89-1.32 1.25-2.04a.07.07 0 0 0-.04-.1 13.13 13.13 0 0 1-1.9-.91.07.07 0 0 1 0-.12l.38-.29a.07.07 0 0 1 .07 0c3.96 1.8 8.24 1.8 12.15 0a.07.07 0 0 1 .08 0l.38.3a.07.07 0 0 1 0 .11 12.5 12.5 0 0 1-1.9.92.07.07 0 0 0-.04.09c.37.72.78 1.4 1.25 2.04a.07.07 0 0 0 .08.03A19.84 19.84 0 0 0 24 18.11a.07.07 0 0 0 .03-.05c.5-5.18-.84-9.67-3.68-13.67a.06.06 0 0 0-.03-.02ZM8.02 15.33c-1.18 0-2.16-1.08-2.16-2.42 0-1.33.95-2.42 2.16-2.42 1.22 0 2.18 1.1 2.16 2.42 0 1.34-.95 2.42-2.16 2.42Zm7.97 0c-1.18 0-2.16-1.08-2.16-2.42 0-1.33.95-2.42 2.16-2.42 1.22 0 2.18 1.1 2.16 2.42 0 1.34-.94 2.42-2.16 2.42Z" />
    </svg>
  );
}

function NavIconLink({
  href,
  label,
  children,
}: {
  href: string;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      aria-label={label}
      className="text-muted-foreground hover:text-foreground focus-visible:ring-ring inline-flex size-9 shrink-0 items-center justify-center rounded-md transition-colors focus-visible:ring-2 focus-visible:outline-none"
    >
      {children}
    </a>
  );
}

export function Nav() {
  const [scrolled, setScrolled] = useState(false);

  useEffect(() => {
    const onScroll = () =>
      setScrolled(window.scrollY > window.innerHeight * 0.8);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <header className="fixed inset-x-0 top-0 z-50 flex justify-center px-[5%] pt-4 md:px-[25%]">
      <div className="bg-background/70 flex w-full items-center justify-between rounded-full border border-foreground/10 px-4 py-2 backdrop-blur-md">
        <Link
          href="/"
          aria-label="ctx home"
          className={`${blobLogo.className} text-foreground text-xl leading-none font-medium transition-opacity duration-200 ${scrolled ? "opacity-100" : "pointer-events-none opacity-0"
            }`}
        >
          ctx
        </Link>

        <div className="flex items-center">
          <NavigationMenu
            className={`${berkeleyMono.className} [&_[data-slot=navigation-menu-link]]:text-xs [&_[data-slot=navigation-menu-link]]:transition-colors [&_[data-slot=navigation-menu-link]]:hover:text-[var(--accent-highlight)] [&_[data-slot=navigation-menu-trigger]]:h-8 [&_[data-slot=navigation-menu-trigger]]:px-2 [&_[data-slot=navigation-menu-trigger]]:py-1 [&_[data-slot=navigation-menu-trigger]]:text-xs`}
          >
            <NavigationMenuList>
              <NavigationMenuItem>
                <NavigationMenuLink render={<Link href="/#install" />}>
                  Install
                </NavigationMenuLink>
              </NavigationMenuItem>

              <NavigationMenuItem>
                <NavigationMenuLink
                  href="https://github.com/satoricorp/ctx/blob/main/docs/ctx-spec-v0.2-draft.md"
                  target="_blank"
                  rel="noreferrer"
                >
                  Spec
                </NavigationMenuLink>
              </NavigationMenuItem>
            </NavigationMenuList>
          </NavigationMenu>
          <div className="ml-4 flex items-center gap-1">
            <div className="bg-foreground/10 h-5 w-px" aria-hidden />
            <NavIconLink href={GITHUB_URL} label="Open ctx on GitHub">
              <GitHubIcon className="size-4" />
            </NavIconLink>
            <NavIconLink href={DISCORD_URL} label="Join the ctx Discord">
              <DiscordIcon className="size-4" />
            </NavIconLink>
            <div className="bg-foreground/10 h-5 w-px" aria-hidden />
            <ThemeToggle />
          </div>
        </div>
      </div>
    </header>
  );
}
