import { AlertColor } from "@mui/material";
import { atom } from "jotai";

export type Severity = AlertColor;

const notificationVisibleAtom = atom(false);
export const notificationConfigAtom = atom({
  title: "",
  message: "",
  severity: "info" as Severity,
});

export const closeNotificationAtom = atom(
  (get) => get(notificationVisibleAtom),
  (_get, set) => {
    set(notificationVisibleAtom, false);
  }
);

export const notifyErrorAtom = atom(null, (_get, set, err: unknown) => {
  let msg: string;
  if (err instanceof Error) {
    msg = err.message;
  } else if (typeof err === "string") {
    msg = err;
  } else {
    msg = "Unknown error";
  }

  set(notificationVisibleAtom, true);
  set(notificationConfigAtom, {
    title: "Error",
    message: msg,
    severity: "error",
  });
});
