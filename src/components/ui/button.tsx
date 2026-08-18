import { Button as BaseButton } from "@base-ui/react/button";
import { forwardRef, type ComponentPropsWithoutRef } from "react";

import { cn } from "@/lib/utils";

type ButtonProps = ComponentPropsWithoutRef<typeof BaseButton>;
type ButtonVariant = "primary" | "ghost" | "destructive";

type StyledButtonProps = ButtonProps & {
  variant?: ButtonVariant;
};

export const Button = forwardRef<HTMLButtonElement, StyledButtonProps>(function Button(
  { className, variant = "primary", ...props },
  ref,
) {
  return (
    <BaseButton
      {...props}
      className={cn(
        "inline-flex h-10 items-center justify-center rounded-lg px-4 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
        variant === "ghost"
          ? "bg-transparent text-foreground hover:bg-muted"
          : variant === "destructive"
            ? "bg-destructive text-white shadow-sm hover:bg-destructive/90"
            : "bg-primary text-primary-foreground shadow-sm hover:bg-primary/90",
        typeof className === "string" ? className : undefined,
      )}
      ref={ref}
    />
  );
});
