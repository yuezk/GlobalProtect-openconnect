import {
  Alert,
  AlertColor,
  AlertTitle,
  Slide,
  SlideProps,
  Snackbar,
  SnackbarCloseReason,
} from "@mui/material";

type TransitionProps = Omit<SlideProps, "direction">;

function TransitionDown(props: TransitionProps) {
  return <Slide {...props} direction="down" />;
}

export type NotificationType = AlertColor;
export type NotificationConfig = {
  open: boolean;
  message: string;
  title?: string;
  type?: NotificationType;
};

type NotificationProps = {
  onClose: () => void;
} & NotificationConfig;

export default function Notification(props: NotificationProps) {
  const { open, message, title, type = "info", onClose } = props;

  function handleClose(
    _: React.SyntheticEvent | Event,
    reason?: SnackbarCloseReason
  ) {
    if (reason === "clickaway") {
      return;
    }
    onClose();
  }

  return (
    <Snackbar
      open={open}
      anchorOrigin={{ vertical: "top", horizontal: "center" }}
      autoHideDuration={5000}
      TransitionComponent={TransitionDown}
      onClose={handleClose}
      sx={{
        top: 0,
        left: 0,
        right: 0,
      }}
    >
      <Alert
        severity={type}
        icon={false}
        sx={{
          width: "100%",
          borderRadius: 0,
        }}
      >
        {title && <AlertTitle>{title}</AlertTitle>}
        {message}
      </Alert>
    </Snackbar>
  );
}
