import { LoadingButton } from "@mui/lab";
import { Box, Button, Drawer, TextField, Typography } from "@mui/material";
import { useAtom, useAtomValue } from "jotai";
import { FormEvent, useEffect, useRef } from "react";
import {
  cancelPasswordAuthAtom,
  passwordAtom,
  passwordLoginAtom,
  passwordPreloginAtom,
  usernameAtom,
} from "../../atoms/passwordLogin";

export default function PasswordAuth() {
  const [visible, cancelPasswordAuth] = useAtom(cancelPasswordAuthAtom);
  const { authMessage, labelUsername, labelPassword } =
    useAtomValue(passwordPreloginAtom);
  const [username, setUsername] = useAtom(usernameAtom);
  const [password, setPassword] = useAtom(passwordAtom);
  const [loading, passwordLogin] = useAtom(passwordLoginAtom);
  const usernameRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (visible) {
      setTimeout(() => {
        usernameRef.current?.querySelector("input")?.focus();
      }, 0);
    }
  }, [visible]);

  function handleSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    passwordLogin();
  }

  return (
    <Drawer open={visible} anchor="bottom" variant="temporary">
      <form onSubmit={handleSubmit}>
        <Box display="flex" flexDirection="column" gap={1.5} padding={2}>
          <Typography>{authMessage}</Typography>
          <TextField
            ref={usernameRef}
            label={labelUsername}
            size="small"
            value={username}
            onChange={(e) => setUsername(e.target.value.trim())}
            InputProps={{ readOnly: loading }}
          />
          <TextField
            label={labelPassword}
            size="small"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            InputProps={{ readOnly: loading }}
          />
          <Box display="flex" gap={1.5}>
            <Button
              variant="outlined"
              onClick={cancelPasswordAuth}
              sx={{ flex: 1 }}
            >
              Cancel
            </Button>
            <LoadingButton
              variant="contained"
              type="submit"
              loading={loading}
              disabled={loading}
            >
              Login
            </LoadingButton>
          </Box>
        </Box>
      </form>
    </Drawer>
  );
}
