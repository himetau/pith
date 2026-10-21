// The renderer's whole view of the machine. Every call goes through main
import { contextBridge, ipcRenderer } from "electron";

const inv = (ch: string) => (...a: unknown[]) => ipcRenderer.invoke(ch, ...a);

const api = {
  start: inv("pi:start"), cmd: inv("pi:cmd"), ui: inv("pi:ui"), stop: inv("pi:stop"), setMode: inv("pi:mode"), live: inv("pi:live"),
  sessions: inv("sessions:list"), meta: inv("meta:all"), setMeta: inv("meta:set"), trashSession: inv("sessions:trash"), usage: inv("usage:get"),
  settings: inv("settings:get"), saveSettings: inv("settings:set"), sys: inv("sys:info"),
  gitStatus: inv("git:status"), gitDiff: inv("git:diff"), gitBranch: inv("git:branch"),
  listFiles: inv("fs:list"), read: inv("fs:read"), write: inv("fs:write"), exists: inv("fs:exists"), pickDir: inv("fs:pickDir"), pickFiles: inv("fs:pickFiles"),
  watchPreview: inv("preview:watch"), unwatchPreview: inv("preview:unwatch"), attachBrowser: inv("browser:attach"),
  agentDefs: inv("agents:defs"), saveAgent: inv("agents:save"), agentHub: inv("agents:hub"),
  images: inv("images:list"), generateImage: inv("images:generate"),
  openExternal: inv("open:external"), openPath: inv("open:path"), reveal: inv("open:reveal"),
  copy: inv("clip:write"), copyImage: inv("clip:image"), notify: inv("notify"), focused: inv("win:focused"),
  termOpen: inv("term:open"), termWrite: inv("term:write"), termResize: inv("term:resize"), termClose: inv("term:close"),
  minimize: inv("win:minimize"), maximize: inv("win:maximize"), close: inv("win:close"),
  on(ch: string, fn: (...a: any[]) => void) {
    const l = (_e: unknown, ...a: any[]) => fn(...a);
    ipcRenderer.on(ch, l);
    return () => { ipcRenderer.removeListener(ch, l); };
  },
};

contextBridge.exposeInMainWorld("pith", api);
export type PithApi = typeof api;
