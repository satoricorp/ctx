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
              <NavigationMenuLink render={<Link href="/#use" />}>
                Use
              </NavigationMenuLink>
            </NavigationMenuItem>

            <NavigationMenuItem>
              <NavigationMenuLink render={<Link href="/#benefits" />}>
                Benefits
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

            <NavigationMenuItem>
              <NavigationMenuLink render={<Link href="/#contribute" />}>
                Contribute
              </NavigationMenuLink>
            </NavigationMenuItem>

          </NavigationMenuList>
        </NavigationMenu>
      </div>
    </header>
  );
}
