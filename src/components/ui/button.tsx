import { Button as BaseButton } from "@base-ui/react/button";
import { forwardRef, type ComponentPropsWithoutRef } from "react";

import { cn } from "@/lib/utils";

type ButtonProps = ComponentPropsWithoutRef<typeof BaseButton>;

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { className, ...props },
  ref,
) {
  return (
    <BaseButton
      {...props}
      className={cn(
        "inline-flex h-10 items-center justify-center rounded-lg bg-primary px-4 text-sm font-medium text-primary-foreground shadow-sm transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
        typeof className === "string" ? className : undefined,
      )}
      ref={ref}
    />
  );
});
