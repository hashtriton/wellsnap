// Tiny i18n: nested dictionaries per language, `{name}` interpolation and CLDR plural forms through
// Intl.PluralRules (Russian needs one/few/many: 1 снимок, 2 снимка, 5 снимков).
import { useEffect, useState } from "react";
import { en } from "../locales/en";
import { ru } from "../locales/ru";
import { api, events, type Lang, type LanguagePref } from "./ipc";

export type Dict = { [key: string]: string | Dict };
export type Params = Record<string, string | number>;
export type TFunction = (key: string, params?: Params) => string;

const dictionaries: Record<Lang, Dict> = { ru, en };

export function resolveLang(pref: LanguagePref, systemLocale: string | undefined): Lang {
  if (pref !== "auto") return pref;
  const primary = (systemLocale ?? "").toLowerCase().split(/[-_]/)[0];
  return ["ru", "be", "uk", "kk", "ky", "uz", "tg", "hy", "az"].includes(primary) ? "ru" : "en";
}

function lookup(dict: Dict, key: string): string | Dict | undefined {
  let node: string | Dict | undefined = dict;
  for (const part of key.split(".")) {
    if (node === undefined || typeof node === "string") return undefined;
    node = node[part];
  }
  return node;
}

function interpolate(text: string, params?: Params): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}

export function createT(lang: Lang): TFunction {
  const plural = new Intl.PluralRules(lang);
  return (key, params) => {
    let node = lookup(dictionaries[lang], key) ?? lookup(dictionaries.en, key);
    if (node !== undefined && typeof node !== "string" && params && typeof params.count === "number") {
      const forms = node;
      node = forms[plural.select(params.count)] ?? forms.other;
    }
    return typeof node === "string" ? interpolate(node, params) : key;
  };
}

/** Current language and `t` for a window; follows "settings:changed". */
export function useI18n(): { lang: Lang; t: TFunction } {
  const [lang, setLang] = useState<Lang>(() => resolveLang("auto", navigator.language));
  useEffect(() => {
    let alive = true;
    api.getSettings().then((s) => alive && setLang(resolveLang(s.language, navigator.language)));
    const unlisten = events.onSettingsChanged((s) => setLang(resolveLang(s.language, navigator.language)));
    return () => {
      alive = false;
      unlisten.then((f) => f());
    };
  }, []);
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  return { lang, t: createT(lang) };
}
