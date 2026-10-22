import { api } from "../shared/ipc";
import { copyText } from "../shared/ui/clipboard";

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
