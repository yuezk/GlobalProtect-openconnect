import {
  ExitToApp,
  GitHub,
  LockReset,
  Menu as MenuIcon,
  Settings,
  VpnLock,
} from "@mui/icons-material";
import { Box, Divider, IconButton, Menu, MenuItem } from "@mui/material";
import { alpha, styled } from "@mui/material/styles";
import { useAtomValue, useSetAtom } from "jotai";
import { useState } from "react";
import { openGatewaySwitcherAtom } from "../../atoms/gateway";
import { quitAtom, resetAtom } from "../../atoms/menu";
import { isProcessingAtom, statusAtom } from "../../atoms/status";

const MenuContainer = styled(Box)(({ theme }) => ({
  position: "absolute",
  zIndex: 1,
  left: theme.spacing(1),
  top: theme.spacing(1),
}));

const StyledMenu = styled(Menu)(({ theme }) => ({
  "& .MuiPaper-root": {
    borderRadius: 6,
    minWidth: 180,
    "& .MuiMenu-list": {
      padding: "4px 0",
    },
    "& .MuiMenuItem-root": {
      minHeight: "auto",
      "& .MuiSvgIcon-root": {
        fontSize: 18,
        color: theme.palette.text.secondary,
        marginRight: theme.spacing(1.5),
      },
      "&:active": {
        backgroundColor: alpha(
          theme.palette.primary.main,
          theme.palette.action.selectedOpacity
        ),
      },
    },
  },
}));

export default function MainMenu() {
  const isProcessing = useAtomValue(isProcessingAtom);
  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);
  const openGatewaySwitcher = useSetAtom(openGatewaySwitcherAtom);
  const status = useAtomValue(statusAtom);
  const reset = useSetAtom(resetAtom);
  const quit = useSetAtom(quitAtom);

  const open = Boolean(anchorEl);
  const handleClick = (event: React.MouseEvent<HTMLElement>) => {
    setAnchorEl(event.currentTarget);
  };
  const handleClose = () => {
    setAnchorEl(null);
  };

  return (
    <>
      <MenuContainer>
        <IconButton onClick={handleClick} disabled={isProcessing}>
          <MenuIcon />
        </IconButton>
        <StyledMenu
          anchorEl={anchorEl}
          open={open}
          onClose={handleClose}
          onClick={handleClose}
        >
          <MenuItem onClick={openGatewaySwitcher} disableRipple>
            <VpnLock />
            Switch Gateway
          </MenuItem>
          <MenuItem onClick={handleClose} disableRipple>
            <Settings />
            Settings
          </MenuItem>
          <MenuItem
            onClick={reset}
            disableRipple
            disabled={status !== "disconnected"}
          >
            <LockReset />
            Reset
          </MenuItem>
          <Divider />
          <MenuItem onClick={quit} disableRipple>
            <ExitToApp />
            Quit
          </MenuItem>
        </StyledMenu>
      </MenuContainer>
      <IconButton
        href="https://github.com/yuezk/GlobalProtect-openconnect"
        target="_blank"
        sx={{
          position: "absolute",
          zIndex: 1,
          right: (theme) => theme.spacing(1),
          top: (theme) => theme.spacing(1),
        }}
      >
        <GitHub />
      </IconButton>
    </>
  );
}
