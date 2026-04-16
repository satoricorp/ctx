"use client";

import localFont from "next/font/local";
import { useState } from "react";

const blobLogo = localFont({
  src: "../public/fonts/Blob.woff2",
  display: "swap",
});

export function Logo() {
  const [offset, setOffset] = useState({ x: 0, y: 0 });

  const handleMove = (e: React.MouseEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const nx = (e.clientX - rect.left - rect.width / 2) / (rect.width / 2);
    const ny = (e.clientY - rect.top - rect.height / 2) / (rect.height / 2);
    setOffset({ x: -nx, y: -ny });
  };

  const handleLeave = () => setOffset({ x: 0, y: 0 });

  return (
    <div
      className="relative"
      onMouseMove={handleMove}
      onMouseLeave={handleLeave}
    >
      <p
        aria-hidden
        className={`${blobLogo.className} text-foreground pointer-events-none absolute inset-0 text-[180px] leading-none font-medium transition-transform duration-300 ease-out`}
        style={{
          WebkitTextStroke: "2px currentColor",
          WebkitTextFillColor: "transparent",
          transform: `translate(${offset.x * 32}px, ${offset.y * 32}px)`,
        }}
      >
        ctx
      </p>
      <p
        aria-hidden
        className={`${blobLogo.className} text-foreground pointer-events-none absolute inset-0 text-[180px] leading-none font-medium transition-transform duration-300 ease-out`}
        style={{
          WebkitTextStroke: "2px currentColor",
          WebkitTextFillColor: "transparent",
          transform: `translate(${offset.x * 16}px, ${offset.y * 16}px)`,
        }}
      >
        ctx
      </p>
      <p
        className={`${blobLogo.className} text-foreground relative text-[180px] leading-none font-medium`}
      >
        ctx
      </p>
    </div>
  );
}
