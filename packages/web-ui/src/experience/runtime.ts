import type { CSSProperties } from "react";

import {
  STANDARD_EXPERIENCE_PACK,
  resolveExperiencePack,
  themeCssVariables,
} from "./index";
import type { WebExperiencePack } from "./types";

declare global {
  interface Window {
    __RINTAWA_WEB_EXPERIENCE_PACK__?: unknown;
  }
}

export function selectedExperiencePack(): WebExperiencePack {
  if (typeof window === "undefined") {
    return STANDARD_EXPERIENCE_PACK;
  }

  try {
    return resolveExperiencePack(window.__RINTAWA_WEB_EXPERIENCE_PACK__);
  } catch (error) {
    console.error("Invalid Rintawa Web experience pack, using standard pack", error);
    return STANDARD_EXPERIENCE_PACK;
  }
}

export function experiencePackStyle(pack: WebExperiencePack): CSSProperties {
  return themeCssVariables(pack) as CSSProperties;
}
