package main

import (
	"encoding/binary"
	"testing"
)

func TestEmbeddedTrayIconIsValidICO(t *testing.T) {
	if len(trayIcon) < 6 {
		t.Fatal("embedded tray icon is too small")
	}
	if got := binary.LittleEndian.Uint16(trayIcon[0:2]); got != 0 {
		t.Fatalf("ICO reserved field = %d, want 0", got)
	}
	if got := binary.LittleEndian.Uint16(trayIcon[2:4]); got != 1 {
		t.Fatalf("ICO type = %d, want 1", got)
	}
	count := int(binary.LittleEndian.Uint16(trayIcon[4:6]))
	if count < 1 {
		t.Fatal("embedded tray icon has no images")
	}
	if len(trayIcon) < 6+16*count {
		t.Fatalf("embedded tray icon directory is truncated: count=%d length=%d", count, len(trayIcon))
	}
	for index := 0; index < count; index++ {
		entry := 6 + 16*index
		bytesInRes := int(binary.LittleEndian.Uint32(trayIcon[entry+8 : entry+12]))
		offset := int(binary.LittleEndian.Uint32(trayIcon[entry+12 : entry+16]))
		if bytesInRes <= 0 || offset < 6+16*count || offset > len(trayIcon) || bytesInRes > len(trayIcon)-offset {
			t.Fatalf("embedded tray icon entry %d is out of bounds", index)
		}
	}
}
