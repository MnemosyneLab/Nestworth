import { open } from "@tauri-apps/plugin-dialog";

export async function pickImagePath(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  return typeof selected === "string" ? selected : null;
}
