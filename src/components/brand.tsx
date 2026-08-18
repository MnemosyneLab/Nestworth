import type { HTMLAttributes } from "react";

import { cn } from "@/lib/utils";

type BrandVariant = "mark" | "wordmark" | "lockup";
type BrandSize = "sm" | "md" | "lg";

const BRAND_ASSETS = {
  mark: "/brand/logo-mark.png",
  wordmark: "/brand/wordmark.png",
} as const;

const MARK_SIZES: Record<BrandSize, string> = {
  sm: "h-8 w-8",
  md: "h-10 w-10",
  lg: "h-14 w-14",
};

const WORDMARK_SIZES: Record<BrandSize, string> = {
  sm: "h-5 w-auto",
  md: "h-7 w-auto",
  lg: "h-10 w-auto",
};

const WORDMARK_DIMENSIONS: Record<BrandSize, { height: number; width: number }> = {
  sm: { height: 20, width: 100 },
  md: { height: 28, width: 140 },
  lg: { height: 40, width: 200 },
};

export function Brand({
  className,
  size = "md",
  variant = "lockup",
  ...props
}: HTMLAttributes<HTMLSpanElement> & {
  size?: BrandSize;
  variant?: BrandVariant;
}) {
  if (variant === "mark") {
    return (
      <span {...props} className={cn("inline-flex shrink-0", className)}>
        <img
          alt="Nestworth"
          className={cn("object-contain", MARK_SIZES[size])}
          height={size === "sm" ? 32 : size === "md" ? 40 : 56}
          src={BRAND_ASSETS.mark}
          width={size === "sm" ? 32 : size === "md" ? 40 : 56}
        />
      </span>
    );
  }

  if (variant === "wordmark") {
    const wordmarkDimensions = WORDMARK_DIMENSIONS[size];
    return (
      <span {...props} className={cn("inline-flex shrink-0", className)}>
        <img
          alt="Nestworth"
          className={cn("object-contain", WORDMARK_SIZES[size])}
          height={wordmarkDimensions.height}
          src={BRAND_ASSETS.wordmark}
          width={wordmarkDimensions.width}
        />
      </span>
    );
  }

  const wordmarkDimensions = WORDMARK_DIMENSIONS[size];

  return (
    <span {...props} className={cn("inline-flex items-center gap-2", className)}>
      <img
        alt=""
        aria-hidden="true"
        className={cn("object-contain", MARK_SIZES[size])}
        height={size === "sm" ? 32 : size === "md" ? 40 : 56}
        src={BRAND_ASSETS.mark}
        width={size === "sm" ? 32 : size === "md" ? 40 : 56}
      />
      <img
        alt="Nestworth"
        className={cn("object-contain", WORDMARK_SIZES[size])}
        height={wordmarkDimensions.height}
        src={BRAND_ASSETS.wordmark}
        width={wordmarkDimensions.width}
      />
    </span>
  );
}
