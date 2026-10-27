import type { strip as ru } from "../ru/strip";

export const strip: typeof ru = {
  region: "Screenshot set",
  moveHint: "Drag to move the strip",
  noComments: "no comments",
  snapshot: "Screenshot {letter}",
  remove: "Remove screenshot {letter} from the set",
  pinHint: "Click to pin it on top of windows",
  copyAll: "Copy all",
  copyAllHint:
    "Copy every screenshot as files: one paste sends them as one album. Comments drawn on the images go with them; to send separate comment text too, use paste with text",
  copyAllHintCtrlV: "Copy every screenshot as files. {keys} pastes them into a chat as one album with the comment text",
  newSnapshot: "New screenshot",
  newSnapshotHint: "Take the next screenshot for this set",
  saveAll: "Save all",
  saveAllHint: "Save every screenshot to the screenshots folder",
  copyMode: "How to copy",
  modeAlbum: "Album",
  modePicture: "One image",
  modeAlbumHint: "Each screenshot as its own file. Telegram and chats show an album",
  modePictureHint:
    "All screenshots are joined into one image, headed \"Screenshot A\", \"Screenshot B\", with the comments under each. Good for mail, WhatsApp, MAX and forms that take a single image",
  copyAllPictureHint: "Copy the set as one image",
  saveAllPictureHint: "Save the set as one image to the screenshots folder",
  clear: "Clear",
  clearHint: "Remove all screenshots from the strip",
  clearConfirm: "Remove all?",
  clearCancel: "Keep",
  smartPaste: "paste with text",
  smartPasteHint: "Pastes every screenshot and the comment text into the open chat in one go",
  smartPasteOff: "The {hotkey} shortcut did not register: another app may be using it. Click to change it in Settings.",
  failed: "That did not work, please try again",
};
