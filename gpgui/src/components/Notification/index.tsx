import {
  Alert,
  AlertTitle,
  Box,
  Link,
  Slide,
  SlideProps,
  Snackbar,
} from "@mui/material";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { openSettingsAtom } from "../../atoms/menu";
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
  const openSettings = useSetAtom(openSettingsAtom);

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
        {message && (
          <Box data-tauri-drag-region>
            {message}
            {/* Guide the user to enable custom OpenSSL settings when encountered the SSL Error */}
            {title === "SSL Error" && (
              <Box mt={1}>
                <Link
                  component="button"
                  variant="body2"
                  onClick={() => openSettings("openssl")}
                >
                  Click here to configure
                </Link>
              </Box>
            )}
          </Box>
        )}
      </Alert>
    </Snackbar>
  );
}
