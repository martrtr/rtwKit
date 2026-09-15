import { cp, mkdir, rm } from "node:fs/promises";

const output = new URL("../build/rtw/", import.meta.url);
const metadata = new URL("../rtw/", import.meta.url);
const web = new URL("../dist/", import.meta.url);

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await cp(metadata, output, { recursive: true });
await cp(web, new URL("web/", output), { recursive: true });
