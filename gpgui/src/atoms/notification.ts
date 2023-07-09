import { AlertColor } from "@mui/material";
import { atom } from "jotai";
import ErrorWithTitle from "../utils/ErrorWithTitle";

export type Severity = AlertColor;

type NotificationConfig = {
  title: string;
  message: string;
  severity: Severity;
  duration?: number;
};

const notificationVisibleAtom = atom(false);
export const notificationConfigAtom = atom<NotificationConfig>({
  title: "",
  message: "",
  severity: "info" as Severity,
  duration: 5000,
});

export const closeNotificationAtom = atom(
  (get) => get(notificationVisibleAtom),
  (_get, set) => {
    set(notificationVisibleAtom, false);
  }
);

export const notifyErrorAtom = atom(
  null,
  (_get, set, err: unknown, duration: number = 5000) => {
    let msg: string;
    if (err instanceof Error) {
      msg = err.message;
    } else if (typeof err === "string") {
      msg = err;
    } else {
      msg = "Unknown error";
    }

    const title = err instanceof ErrorWithTitle ? err.title : "Error";

    set(notificationVisibleAtom, true);
    set(notificationConfigAtom, {
      title,
      message: msg,
      severity: "error",
      duration: duration <= 0 ? undefined : duration,
    });
  }
);

export const notifySuccessAtom = atom(
  null,
  (_get, set, msg: string, duration: number = 5000) => {
    set(notificationVisibleAtom, true);
    set(notificationConfigAtom, {
      title: "Success",
      message: msg,
      severity: "success",
      duration: duration <= 0 ? undefined : duration,
    });
  }
);
