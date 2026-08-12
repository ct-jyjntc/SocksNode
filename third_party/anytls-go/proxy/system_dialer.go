package proxy

import (
	"net"
	"os"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/sirupsen/logrus"
)

// SystemDialer is used for all outbound TCP from anytls-client to the upstream.
// SocksNode sets LocalAddr / interface bind so traffic leaves via a physical NIC
// instead of Surge/Clash utun when possible.
var SystemDialer = &net.Dialer{
	Timeout: time.Second * 5,
}

// ConfigureSystemDialer binds outbound dials to a local IPv4 and optionally a
// macOS interface index (IP_BOUND_IF). bindIP empty => no LocalAddr override.
// ifIndex 0 => skip IP_BOUND_IF.
func ConfigureSystemDialer(bindIP string, ifIndex uint32) {
	d := &net.Dialer{
		Timeout: time.Second * 5,
	}
	if ip := net.ParseIP(strings.TrimSpace(bindIP)); ip != nil {
		d.LocalAddr = &net.TCPAddr{IP: ip, Port: 0}
		logrus.Infoln("[Client] outbound bind IP:", ip.String())
	}
	if ifIndex > 0 && runtime.GOOS == "darwin" {
		idx := ifIndex
		d.Control = func(network, address string, c syscall.RawConn) error {
			var opErr error
			err := c.Control(func(fd uintptr) {
				// IP_BOUND_IF = 25 on Darwin for IPv4.
				// IPV6_BOUND_IF = 125 for IPv6.
				level := syscall.IPPROTO_IP
				opt := 25
				if strings.Contains(network, "6") {
					level = syscall.IPPROTO_IPV6
					opt = 125
				}
				opErr = syscall.SetsockoptInt(int(fd), level, opt, int(idx))
			})
			if err != nil {
				return err
			}
			return opErr
		}
		logrus.Infoln("[Client] outbound IP_BOUND_IF ifindex:", idx)
	}
	SystemDialer = d
}

// DetectPhysicalIPv4 picks a non-loopback, non-utun IPv4 suitable for bind.
// Prefer $SOCKSNODE_BIND_IP if set.
func DetectPhysicalIPv4() (ip string, ifName string) {
	if v := strings.TrimSpace(os.Getenv("SOCKSNODE_BIND_IP")); v != "" {
		if net.ParseIP(v) != nil {
			return v, "env"
		}
	}
	ifaces, err := net.Interfaces()
	if err != nil {
		return "", ""
	}
	type cand struct {
		ip, name string
		score    int
	}
	var list []cand
	for _, iface := range ifaces {
		name := strings.ToLower(iface.Name)
		if iface.Flags&net.FlagUp == 0 || iface.Flags&net.FlagLoopback != 0 {
			continue
		}
		if isVirtualIF(name) {
			continue
		}
		addrs, err := iface.Addrs()
		if err != nil {
			continue
		}
		for _, a := range addrs {
			var ipNet *net.IPNet
			switch v := a.(type) {
			case *net.IPNet:
				ipNet = v
			case *net.IPAddr:
				ipNet = &net.IPNet{IP: v.IP}
			}
			if ipNet == nil || ipNet.IP == nil {
				continue
			}
			v4 := ipNet.IP.To4()
			if v4 == nil || v4.IsLinkLocalUnicast() {
				continue
			}
			score := 10
			if name == "en0" {
				score = 0
			} else if name == "en1" {
				score = 1
			} else if strings.HasPrefix(name, "en") {
				score = 2
			}
			list = append(list, cand{ip: v4.String(), name: iface.Name, score: score})
		}
	}
	if len(list) == 0 {
		return "", ""
	}
	best := list[0]
	for _, c := range list[1:] {
		if c.score < best.score {
			best = c
		}
	}
	return best.ip, best.name
}

func isVirtualIF(name string) bool {
	prefixes := []string{
		"utun", "awdl", "llw", "bridge", "ap", "gif", "stf", "anpi", "anri",
		"nan", "ipsec", "ppp", "vmnet", "veth", "docker", "tun", "tap", "wg",
	}
	for _, p := range prefixes {
		if name == p || strings.HasPrefix(name, p) {
			return true
		}
	}
	return name == "lo" || name == "lo0"
}

// IfIndexByName returns interface index or 0.
func IfIndexByName(name string) uint32 {
	if name == "" || name == "env" {
		return 0
	}
	iface, err := net.InterfaceByName(name)
	if err != nil {
		return 0
	}
	return uint32(iface.Index)
}

// ParseIfIndexEnv reads SOCKSNODE_IFINDEX if set.
func ParseIfIndexEnv() uint32 {
	v := strings.TrimSpace(os.Getenv("SOCKSNODE_IFINDEX"))
	if v == "" {
		return 0
	}
	n, err := strconv.ParseUint(v, 10, 32)
	if err != nil {
		return 0
	}
	return uint32(n)
}
