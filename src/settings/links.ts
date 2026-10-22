import { REPO_URL } from "../shared/donate";
import { api, type Lang } from "../shared/ipc";
import { copyText } from "../shared/ui/clipboard";

/** Public list of apps where paste with text was checked, in the UI language. */
export function pasteTargetsUrl(lang: Lang): string {
  return `${REPO_URL}/blob/main/docs/paste-targets${lang === "ru" ? ".ru" : ""}.md`;
}

/** Opens a link in the browser; if that fails the link goes to the clipboard and false is returned. */
export async function openLink(url: string): Promise<boolean> {
  try {
    await api.openExternal(url);
    return true;
  } catch {
    await copyText(url);
    return false;
  }
}
