import { Alert, AlertTitle, Box, Slide, SlideProps, Snackbar } from "@mui/material";
import { useAtom, useAtomValue } from "jotai";
import {
  closeNotificationAtom,
  notificationConfigAtom,
} from "../../atoms/notification";

type TransitionProps = Omit<SlideProps, "direction">;
function TransitionDown(props: TransitionProps) {
  return <Slide {...props} direction="down" />;
}

export default function Notification() {
  const { title, message, severity, duration } = useAtomValue(
    notificationConfigAtom
  );
  const [visible, closeNotification] = useAtom(closeNotificationAtom);
  const handleClose = () => {
    if (duration) {
      closeNotification();
    }
  };

  return (
    <Snackbar
      open={visible}
      anchorOrigin={{ vertical: "top", horizontal: "center" }}
      autoHideDuration={duration}
      TransitionComponent={TransitionDown}
      onClose={handleClose}
      sx={{
        top: 0,
        left: 0,
        right: 0,
      }}
    >
      <Alert
        data-tauri-drag-region
        severity={severity}
        icon={false}
        sx={{
          width: "100%",
          borderRadius: 0,
        }}
      >
        {title && <AlertTitle data-tauri-drag-region>{title}</AlertTitle>}
        {message && <Box data-tauri-drag-region>{message}</Box>}
      </Alert>
    </Snackbar>
  );
}
