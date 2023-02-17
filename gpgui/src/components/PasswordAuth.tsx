import LoadingButton from "@mui/lab/LoadingButton";
import { Box, Button, Drawer, TextField, Typography } from "@mui/material";
import { FormEvent, useEffect, useRef, useState } from "react";
import { Maybe } from "../types";

export type PasswordAuthData = {
  labelUsername: string;
  labelPassword: string;
  authMessage: Maybe<string>;
};

export type Credentials = {
  username: string;
  password: string;
};

type LoginCallback = (params: Credentials) => void;

type Props = {
  open: boolean;
  authData: PasswordAuthData | undefined;
  authenticating: boolean;
  onCancel: () => void;
  onLogin: LoginCallback;
};

type AuthFormProps = {
  authenticating: boolean;
  onCancel: () => void;
  onSubmit: LoginCallback;
} & PasswordAuthData;

function AuthForm(props: AuthFormProps) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const inputRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    inputRef.current?.querySelector("input")?.focus();
  }, []);

  const {
    authenticating,
    authMessage,
    labelUsername,
    labelPassword,
    onCancel,
    onSubmit,
  } = props;

  function handleSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();

    if (username.trim() === "" || password === "") {
      return;
    }

    onSubmit({ username, password });
  }

  return (
    <form onSubmit={handleSubmit}>
      <Box display="flex" flexDirection="column" gap={1.5} padding={2}>
        <Typography>{authMessage}</Typography>
        <TextField
          ref={inputRef}
          label={labelUsername}
          size="small"
          autoFocus
          value={username}
          InputProps={{ readOnly: authenticating }}
          onChange={(e) => setUsername(e.target.value)}
        />
        <TextField
          label={labelPassword}
          size="small"
          type="password"
          value={password}
          InputProps={{ readOnly: authenticating }}
          onChange={(e) => setPassword(e.target.value)}
        />
        <Box display="flex" gap={1.5}>
          <Button
            variant="outlined"
            sx={{ flex: 1, textTransform: "none" }}
            onClick={onCancel}
          >
            Cancel
          </Button>
          <LoadingButton
            loading={authenticating}
            variant="contained"
            sx={{ flex: 1, textTransform: "none" }}
            type="submit"
            disabled={authenticating}
          >
            Login
          </LoadingButton>
        </Box>
      </Box>
    </form>
  );
}

export default function PasswordAuth(props: Props) {
  const { open, authData, authenticating, onCancel, onLogin } = props;

  return (
    <Drawer anchor="bottom" variant="temporary" open={open}>
      {authData && (
        <AuthForm
          {...authData}
          authenticating={authenticating}
          onCancel={onCancel}
          onSubmit={onLogin}
        />
      )}
    </Drawer>
  );
}
