import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { MediaImage } from "@/features/media/media-image";
import { GhostButton } from "@/features/references/reference-page";
import { commands, type CommandError } from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { pickImagePath } from "@/lib/tauri/pick-image";

type MediaKind = "avatar" | "logo";

export function ImagePicker({
  assetId,
  entityId,
  kind,
  onSaved,
}: {
  assetId: string | null;
  entityId: string;
  kind:
    "memberAvatar" | "institutionLogo" | "groupLogo" | "accountLogo" | "instrumentLogo";
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [error, setError] = useState<CommandError | null>(null);
  const mutation = useMutation({
    mutationFn: async (path: string) => {
      const input = { id: entityId, path };
      if (kind === "memberAvatar") {
        return unwrapResult(commands.setMemberAvatar(input));
      }
      if (kind === "institutionLogo") {
        return unwrapResult(commands.setInstitutionLogo(input));
      }
      if (kind === "groupLogo") {
        return unwrapResult(commands.setGroupLogo(input));
      }
      if (kind === "instrumentLogo") {
        return unwrapResult(commands.setInstrumentLogo(input));
      }
      return unwrapResult(commands.setAccountLogo(input));
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["media"] });
      await onSaved();
    },
    onError: (caught) => setError(commandErrorFromUnknown(caught)),
  });

  const mediaKind: MediaKind = kind === "memberAvatar" ? "avatar" : "logo";

  return (
    <div className="space-y-2">
      <p className="text-sm font-medium">{t(`media.${mediaKind}`)}</p>
      <div className="flex items-center gap-3">
        <MediaImage alt="" assetId={assetId} />
        <GhostButton
          disabled={mutation.isPending}
          onClick={() => {
            void pickImagePath().then((path) => {
              if (!path) {
                return;
              }
              setError(null);
              mutation.mutate(path);
            });
          }}
          type="button"
        >
          {mutation.isPending ? t("references.saving") : t("media.choose")}
        </GhostButton>
      </div>
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
    </div>
  );
}
