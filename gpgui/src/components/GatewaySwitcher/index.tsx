import { Check } from "@mui/icons-material";
import {
  Drawer,
  ListItemIcon,
  ListItemText,
  MenuItem,
  MenuList,
} from "@mui/material";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  gatewaySwitcherVisibleAtom,
  portalGatewaysAtom,
  selectedGatewayAtom,
  switchGatewayAtom,
} from "../../atoms/gateway";
import { GatewayData } from "../../atoms/portal";

export default function GatewaySwitcher() {
  const [visible, setGatewaySwitcherVisible] = useAtom(
    gatewaySwitcherVisibleAtom
  );
  const gateways = useAtomValue(portalGatewaysAtom);
  const selectedGateway = useAtomValue(selectedGatewayAtom)?.name;
  const switchGateway = useSetAtom(switchGatewayAtom);

  const handleClose = () => {
    setGatewaySwitcherVisible(false);
  };

  const handleMenuClick = (gateway: GatewayData) => () => {
    setGatewaySwitcherVisible(false);
    if (gateway.name !== selectedGateway) {
      switchGateway(gateway);
    }
  };

  return (
    <Drawer
      anchor="bottom"
      variant="temporary"
      open={visible}
      onClose={handleClose}
    >
      <MenuList
        sx={{
          maxHeight: 320,
        }}
      >
        {!gateways.length && <MenuItem disabled>No gateways found</MenuItem>}

        {gateways.map(({ name, address }) => (
          <MenuItem key={name} onClick={handleMenuClick({ name, address })}>
            {selectedGateway === name && (
              <ListItemIcon>
                <Check />
              </ListItemIcon>
            )}
            <ListItemText inset={selectedGateway !== name}>{name}</ListItemText>
          </MenuItem>
        ))}
      </MenuList>
    </Drawer>
  );
}
