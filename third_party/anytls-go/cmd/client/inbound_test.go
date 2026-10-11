package main

import (
	"context"
	"strings"
	"testing"

	M "github.com/sagernet/sing/common/metadata"
)

func TestDisabledUDPDenyBeforeOpeningTunnel(t *testing.T) {
	client := &myClient{allowUDP: false}
	err := client.NewPacketConnection(context.Background(), nil, M.Metadata{})
	if err == nil || !strings.Contains(err.Error(), "disabled") {
		t.Fatalf("disabled UDP must reject association, got %v", err)
	}
}
