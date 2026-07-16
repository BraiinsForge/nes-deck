package main

import (
	"embed"
	"strings"
)

//go:embed settings-icons/*.png settings-icons/UPSTREAM.txt
var settingsIconAssets embed.FS

func settingsIconAsset(requestPath string) ([]byte, bool) {
	for _, spec := range knekkoSettingsIconSpecs {
		if spec.imageURL != requestPath {
			continue
		}
		path := strings.TrimPrefix(requestPath, "/assets/")
		asset, err := settingsIconAssets.ReadFile(path)
		return asset, err == nil
	}
	return nil, false
}
