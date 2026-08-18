import { useQuery } from "@tanstack/react-query";

import { commands } from "@/generated/tauri-bindings";
import { unwrapResult } from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

export const mediaQueryKey = (assetId: string) => ["media", assetId] as const;

export function MediaImage({
  alt,
  assetId,
  className,
}: {
  alt: string;
  assetId: string | null;
  className?: string;
}) {
  const media = useQuery({
    enabled: Boolean(assetId),
    queryKey: assetId ? mediaQueryKey(assetId) : ["media", "none"],
    queryFn: () => unwrapResult(commands.getMedia({ assetId: assetId as string })),
    staleTime: Infinity,
  });
  if (!assetId || !media.data) {
    return (
      <span
        aria-hidden={alt ? undefined : true}
        className={cn(
          "inline-flex h-10 w-10 items-center justify-center rounded-lg bg-surface-soft text-muted-foreground",
          className,
        )}
      />
    );
  }
  return (
    <img
      alt={alt}
      className={cn("h-10 w-10 rounded-lg object-cover", className)}
      src={`data:${media.data.mimeType};base64,${media.data.data}`}
    />
  );
}
