"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuTrigger,
} from "@/components/ui/navigation-menu";
import { berkeleyMono, blobLogo } from "./fonts";

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
          className={`${blobLogo.className} text-foreground text-xl leading-none font-medium transition-opacity duration-200 ${
            scrolled ? "opacity-100" : "pointer-events-none opacity-0"
          }`}
        >
          ctx
        </Link>

        <NavigationMenu
          className={`${berkeleyMono.className} [&_[data-slot=navigation-menu-link]]:text-xs [&_[data-slot=navigation-menu-trigger]]:h-8 [&_[data-slot=navigation-menu-trigger]]:px-2 [&_[data-slot=navigation-menu-trigger]]:py-1 [&_[data-slot=navigation-menu-trigger]]:text-xs`}
        >
          <NavigationMenuList>
            <NavigationMenuItem>
              <NavigationMenuLink render={<Link href="/getting-started" />}>
                Getting Started
              </NavigationMenuLink>
            </NavigationMenuItem>

            <NavigationMenuItem>
              <NavigationMenuLink render={<Link href="/how-it-works" />}>
                How It Works
              </NavigationMenuLink>
            </NavigationMenuItem>

            <NavigationMenuItem>
              <NavigationMenuLink render={<Link href="/docs" />}>
                Docs
              </NavigationMenuLink>
            </NavigationMenuItem>

            <NavigationMenuItem>
              <NavigationMenuTrigger>Research</NavigationMenuTrigger>
              <NavigationMenuContent
                className={`${berkeleyMono.className} flex w-72 flex-col p-1`}
              >
                <NavigationMenuLink
                  href="https://arxiv.org/abs/TODO"
                  target="_blank"
                  rel="noreferrer"
                  className="flex-col items-start gap-1"
                >
                  <div className="text-xs font-medium">arXiv</div>
                  <p className="text-muted-foreground text-[11px] leading-snug">
                    Read the paper behind the Portable Context Protocol.
                  </p>
                </NavigationMenuLink>
                <NavigationMenuLink
                  href="https://github.com/TODO/ctx-spec"
                  target="_blank"
                  rel="noreferrer"
                  className="flex-col items-start gap-1"
                >
                  <div className="text-xs font-medium">Spec on GitHub</div>
                  <p className="text-muted-foreground text-[11px] leading-snug">
                    Browse the open specification and reference implementation.
                  </p>
                </NavigationMenuLink>
              </NavigationMenuContent>
            </NavigationMenuItem>
          </NavigationMenuList>
        </NavigationMenu>
      </div>
    </header>
  );
}
