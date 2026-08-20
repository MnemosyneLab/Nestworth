import { readFile } from "node:fs/promises";

const expectedVersion = "0.1.5";
const expectedBun = "1.3.14";

const packageJson = JSON.parse(await readFile("package.json", "utf8")) as {
  version?: string;
  packageManager?: string;
};
const cargoToml = await readFile("src-tauri/Cargo.toml", "utf8");
const cargoLock = await readFile("src-tauri/Cargo.lock", "utf8");
const tauriConfig = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8")) as {
  version?: string;
};

const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const lockedCargoVersion = cargoLock.match(
  /\[\[package\]\]\nname = "nestworth"\nversion = "([^"]+)"/,
)?.[1];
const failures: string[] = [];

if (packageJson.version !== expectedVersion) {
  failures.push(`package.json version is ${packageJson.version ?? "missing"}`);
}
if (packageJson.packageManager !== `bun@${expectedBun}`) {
  failures.push(
    `packageManager is ${packageJson.packageManager ?? "missing"}; expected bun@${expectedBun}`,
  );
}
if (Bun.version !== expectedBun) {
  failures.push(`running Bun ${Bun.version}; expected ${expectedBun}`);
}
if (cargoVersion !== expectedVersion) {
  failures.push(`Cargo.toml version is ${cargoVersion ?? "missing"}`);
}
if (lockedCargoVersion !== expectedVersion) {
  failures.push(`Cargo.lock version is ${lockedCargoVersion ?? "missing"}`);
}
if (tauriConfig.version !== expectedVersion) {
  failures.push(`tauri.conf.json version is ${tauriConfig.version ?? "missing"}`);
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`version ${expectedVersion}; Bun ${expectedBun}`);
