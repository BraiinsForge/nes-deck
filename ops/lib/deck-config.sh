#!/usr/bin/env bash

# Parse the private per-Deck configuration without evaluating its contents.

deck_config_valid_ssh_target() {
  [[ ${1-} =~ ^root@[A-Za-z0-9._:-]+$ ]]
}

deck_config_ipv4_integer() {
  local address=${1-}
  local first second third fourth
  [[ $address =~ ^([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})$ ]] ||
    return 1
  first=${BASH_REMATCH[1]}
  second=${BASH_REMATCH[2]}
  third=${BASH_REMATCH[3]}
  fourth=${BASH_REMATCH[4]}
  local octet
  for octet in "$first" "$second" "$third" "$fourth"; do
    [[ $octet == 0 || $octet != 0* ]] || return 1
    (( 10#$octet <= 255 )) || return 1
  done
  printf '%u\n' "$((
    (10#$first << 24) |
    (10#$second << 16) |
    (10#$third << 8) |
    10#$fourth
  ))"
}

deck_config_valid_wireguard_address() {
  local address=${1-}
  local first=${address%%.*}
  deck_config_ipv4_integer "$address" >/dev/null || return 1
  (( 10#$first >= 1 && 10#$first <= 223 && 10#$first != 127 ))
}

deck_config_valid_wireguard_route() {
  local route=${1-}
  local network prefix network_integer mask
  [[ $route =~ ^([^/]+)/([0-9]|[12][0-9]|3[0-2])$ ]] || return 1
  network=${BASH_REMATCH[1]}
  prefix=${BASH_REMATCH[2]}
  network_integer=$(deck_config_ipv4_integer "$network") || return 1
  if (( 10#$prefix == 0 )); then
    mask=0
  else
    mask=$(( (0xffffffff << (32 - 10#$prefix)) & 0xffffffff ))
  fi
  (( (network_integer & mask) == network_integer ))
}

deck_config_route_contains_address() {
  local route=${1-}
  local address=${2-}
  local network=${route%/*}
  local prefix=${route##*/}
  local network_integer address_integer mask
  deck_config_valid_wireguard_route "$route" || return 1
  address_integer=$(deck_config_ipv4_integer "$address") || return 1
  network_integer=$(deck_config_ipv4_integer "$network") || return 1
  if (( 10#$prefix == 0 )); then
    mask=0
  else
    mask=$(( (0xffffffff << (32 - 10#$prefix)) & 0xffffffff ))
  fi
  (( (address_integer & mask) == network_integer ))
}

deck_config_valid_uploader_password() {
  local password=${1-}
  [[ ${#password} -ge 8 && ${#password} -le 128 &&
     $password != *$'\r'* && $password != *$'\n'* ]]
}

deck_config_valid_recovery_wifi_ssid() {
  local ssid=${1-}
  [[ ${#ssid} -le 32 && $ssid != *$'\r'* && $ssid != *$'\n'* ]]
}

deck_config_load() {
  if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo 'deck_config_load requires CONFIG and an optional SSH target override' >&2
    return 2
  fi

  local path=$1
  local target_override=${2-}
  local mode
  local line
  local key
  local value
  local line_number=0
  local target_seen=0
  local wireguard_seen=0
  local wireguard_route_seen=0
  local wireguard_health_seen=0
  local recovery_wifi_seen=0
  local password_seen=0

  [[ -f $path && ! -L $path ]] || {
    echo "Private Deck configuration is missing or unsafe: $path" >&2
    echo "Create it with: ops/configure-deck.sh $path" >&2
    return 1
  }
  mode=$(stat -c %a -- "$path") || {
    echo "Cannot inspect Deck configuration permissions: $path" >&2
    return 1
  }
  [[ $mode =~ ^[0-7]{3,4}$ ]] || {
    echo "Deck configuration has an unrecognized file mode: $path" >&2
    return 1
  }
  if (( (8#$mode & 077) != 0 )); then
    echo "Deck configuration must not be accessible by group or others: $path" >&2
    return 1
  fi

  DECK_SSH_TARGET=
  DECK_WIREGUARD_ADDRESS=
  DECK_WIREGUARD_ROUTE=
  DECK_WIREGUARD_HEALTH_ADDRESS=
  DECK_RECOVERY_WIFI_SSID=
  ROM_UPLOADER_PASSWORD=
  while IFS= read -r line || [[ -n $line ]]; do
    line_number=$((line_number + 1))
    [[ -z $line || $line == \#* ]] && continue
    [[ $line == *=* ]] || {
      echo "Configuration line $line_number must have the form KEY=VALUE: $path" >&2
      return 1
    }
    key=${line%%=*}
    value=${line#*=}
    case $key in
      DECK_SSH_TARGET)
        [[ $target_seen -eq 0 ]] || {
          echo "Configuration repeats DECK_SSH_TARGET: $path" >&2
          return 1
        }
        DECK_SSH_TARGET=$value
        target_seen=1
        ;;
      DECK_WIREGUARD_ADDRESS)
        [[ $wireguard_seen -eq 0 ]] || {
          echo "Configuration repeats DECK_WIREGUARD_ADDRESS: $path" >&2
          return 1
        }
        DECK_WIREGUARD_ADDRESS=$value
        wireguard_seen=1
        ;;
      DECK_WIREGUARD_ROUTE)
        [[ $wireguard_route_seen -eq 0 ]] || {
          echo "Configuration repeats DECK_WIREGUARD_ROUTE: $path" >&2
          return 1
        }
        DECK_WIREGUARD_ROUTE=$value
        wireguard_route_seen=1
        ;;
      DECK_WIREGUARD_HEALTH_ADDRESS)
        [[ $wireguard_health_seen -eq 0 ]] || {
          echo "Configuration repeats DECK_WIREGUARD_HEALTH_ADDRESS: $path" >&2
          return 1
        }
        DECK_WIREGUARD_HEALTH_ADDRESS=$value
        wireguard_health_seen=1
        ;;
      DECK_RECOVERY_WIFI_SSID)
        [[ $recovery_wifi_seen -eq 0 ]] || {
          echo "Configuration repeats DECK_RECOVERY_WIFI_SSID: $path" >&2
          return 1
        }
        DECK_RECOVERY_WIFI_SSID=$value
        recovery_wifi_seen=1
        ;;
      ROM_UPLOADER_PASSWORD)
        [[ $password_seen -eq 0 ]] || {
          echo "Configuration repeats ROM_UPLOADER_PASSWORD: $path" >&2
          return 1
        }
        ROM_UPLOADER_PASSWORD=$value
        password_seen=1
        ;;
      *)
        echo "Configuration key on line $line_number is not supported: $key" >&2
        return 1
        ;;
    esac
  done <"$path"

  [[ $target_seen -eq 1 ]] || {
    echo "Configuration is missing DECK_SSH_TARGET: $path" >&2
    return 1
  }
  [[ $wireguard_seen -eq 1 ]] || {
    echo "Configuration is missing DECK_WIREGUARD_ADDRESS: $path" >&2
    return 1
  }
  [[ $wireguard_route_seen -eq 1 ]] || {
    echo "Configuration is missing DECK_WIREGUARD_ROUTE: $path" >&2
    return 1
  }
  [[ $wireguard_health_seen -eq 1 ]] || {
    echo "Configuration is missing DECK_WIREGUARD_HEALTH_ADDRESS: $path" >&2
    return 1
  }
  [[ $password_seen -eq 1 ]] || {
    echo "Configuration is missing ROM_UPLOADER_PASSWORD: $path" >&2
    return 1
  }

  if [[ -n $target_override ]]; then
    DECK_SSH_TARGET=$target_override
  fi
  deck_config_valid_ssh_target "$DECK_SSH_TARGET" || {
    echo 'DECK_SSH_TARGET must have the form root@DECK-IP' >&2
    return 1
  }
  deck_config_valid_wireguard_address "$DECK_WIREGUARD_ADDRESS" || {
    echo 'DECK_WIREGUARD_ADDRESS must be a canonical unicast IPv4 address' >&2
    return 1
  }
  deck_config_valid_wireguard_route "$DECK_WIREGUARD_ROUTE" || {
    echo 'DECK_WIREGUARD_ROUTE must be a canonical IPv4 network prefix' >&2
    return 1
  }
  deck_config_route_contains_address \
    "$DECK_WIREGUARD_ROUTE" "$DECK_WIREGUARD_ADDRESS" || {
    echo 'DECK_WIREGUARD_ROUTE must contain DECK_WIREGUARD_ADDRESS' >&2
    return 1
  }
  deck_config_valid_wireguard_address "$DECK_WIREGUARD_HEALTH_ADDRESS" || {
    echo 'DECK_WIREGUARD_HEALTH_ADDRESS must be canonical unicast IPv4' >&2
    return 1
  }
  deck_config_route_contains_address \
    "$DECK_WIREGUARD_ROUTE" "$DECK_WIREGUARD_HEALTH_ADDRESS" || {
    echo 'DECK_WIREGUARD_ROUTE must contain DECK_WIREGUARD_HEALTH_ADDRESS' >&2
    return 1
  }
  [[ $DECK_WIREGUARD_HEALTH_ADDRESS != "$DECK_WIREGUARD_ADDRESS" ]] || {
    echo 'DECK_WIREGUARD_HEALTH_ADDRESS must differ from the Deck address' >&2
    return 1
  }
  deck_config_valid_recovery_wifi_ssid "$DECK_RECOVERY_WIFI_SSID" || {
    echo 'DECK_RECOVERY_WIFI_SSID must contain at most 32 bytes without line breaks' >&2
    return 1
  }
  deck_config_valid_uploader_password "$ROM_UPLOADER_PASSWORD" || {
    echo 'ROM_UPLOADER_PASSWORD must contain 8 through 128 bytes without line breaks' >&2
    return 1
  }
}
