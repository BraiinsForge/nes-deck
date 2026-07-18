#!/usr/bin/env bash

# Parse the private per-Deck configuration without evaluating its contents.

deck_config_valid_ssh_target() {
  [[ ${1-} =~ ^root@[A-Za-z0-9._:-]+$ ]]
}

deck_config_valid_wireguard_address() {
  local address=${1-}
  local peer
  [[ $address =~ ^10\.0\.0\.([1-9][0-9]{0,2})$ ]] || return 1
  peer=${BASH_REMATCH[1]}
  (( 10#$peer >= 2 && 10#$peer <= 253 ))
}

deck_config_valid_uploader_password() {
  local password=${1-}
  [[ ${#password} -ge 8 && ${#password} -le 128 &&
     $password != *$'\r'* && $password != *$'\n'* ]]
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
    echo 'DECK_WIREGUARD_ADDRESS must be a usable 10.0.0.0/24 peer address' >&2
    return 1
  }
  deck_config_valid_uploader_password "$ROM_UPLOADER_PASSWORD" || {
      echo 'ROM_UPLOADER_PASSWORD must contain 8 through 128 bytes without line breaks' >&2
      return 1
    }
}
