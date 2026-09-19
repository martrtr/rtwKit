import type { CSSProperties } from "react";

import {
  STANDARD_EXPERIENCE_PACK,
  applyExperienceModule,
  resolveExperiencePack,
  themeCssVariables,
} from "./index";
import type { WebExperiencePack } from "./types";

declare global {
  interface Window {
    __RINTAWA_WEB_EXPERIENCE_PACK__?: unknown;
    __RINTAWA_WEB_EXPERIENCE_MODULES__?: unknown[];
  }
}

export function selectedExperiencePack(): WebExperiencePack {
  if (typeof window === "undefined") {
    return STANDARD_EXPERIENCE_PACK;
  }

  let pack: WebExperiencePack;
  try {
    pack = resolveExperiencePack(window.__RINTAWA_WEB_EXPERIENCE_PACK__);
  } catch (error) {
    console.error("Invalid Rintawa Web experience pack, using standard pack", error);
    pack = STANDARD_EXPERIENCE_PACK;
  }

  for (const module of window.__RINTAWA_WEB_EXPERIENCE_MODULES__ ?? []) {
    try {
      pack = applyExperienceModule(pack, module);
    } catch (error) {
      console.error("Invalid Rintawa Web experience module, skipping it", error);
    }
  }
  return pack;
}

export function experiencePackStyle(pack: WebExperiencePack): CSSProperties {
  return themeCssVariables(pack) as CSSProperties;
}
