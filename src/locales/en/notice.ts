import type { notice as ru } from "../ru/notice";

export const notice: typeof ru = {
  region: "Reminder to support the project",
  shots: { one: "{count} screenshot", few: "{count} screenshots", many: "{count} screenshots", other: "{count} screenshots" },
  nudge: {
    v0: {
      title: "Enjoying WellSnap?",
      body:
        "You have taken {shots}. WellSnap is free, ad-free and keeps your images on your computer. " +
        "If it saves you time, you can support development with any amount. Every feature stays free either way.",
    },
    v1: {
      title: "{shots} and counting",
      body:
        "We hope WellSnap has become a handy tool. Donations pay for code signing on Windows and macOS, the update server " +
        "and time for new features. If you want to help, the button is right here. If not, we will not ask again for a few months.",
    },
    v2: {
      title: "{shots} already",
      body:
        "That is quite a journey together. If the project is useful to you, you can support it with any amount. " +
        "We ask rarely: the next reminder comes in six months at the earliest.",
    },
    v3: {
      title: "You have been with us a while",
      body:
        "Thank you for using WellSnap so much. If you would like to help the project, you can do it any time in Settings, " +
        "under Support. This is the last regular reminder.",
    },
  },
  support: "Support",
  later: "Later",
  alreadySupported: "I already supported",
  never: "Don't remind me",
  updated: {
    title: "WellSnap updated to version {version}",
    whatsNew: "What's new",
    changelog: "Changelog",
    setCleared: "The snapshot set was cleared during the restart",
    close: "Close",
  },
  crash: {
    region: "Crash report",
    title: "WellSnap closed unexpectedly",
    body: "Send a report to the developer? It contains the app and system versions, where the failure happened and the last lines of the technical log. Screenshots, comments and files are not attached.",
    send: "Send",
    dontSend: "Don't send",
    always: "Always send without asking",
    details: "Details",
  },
};
