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
    return (
      <span {...props} className={cn("inline-flex shrink-0", className)}>
        <img
          alt="Nestworth"
          className={cn("object-contain", WORDMARK_SIZES[size])}
          height={size === "sm" ? 20 : size === "md" ? 28 : 40}
          src={BRAND_ASSETS.wordmark}
          width={size === "sm" ? 60 : size === "md" ? 84 : 120}
        />
      </span>
    );
  }

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
        height={size === "sm" ? 20 : size === "md" ? 28 : 40}
        src={BRAND_ASSETS.wordmark}
        width={size === "sm" ? 60 : size === "md" ? 84 : 120}
      />
    </span>
  );
}
