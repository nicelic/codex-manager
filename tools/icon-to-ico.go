package main

import (
	"encoding/binary"
	"fmt"
	"os"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: go run tools/icon-to-ico.go input.png output.ico")
		os.Exit(2)
	}
	png, err := os.ReadFile(os.Args[1])
	if err != nil {
		fail(err)
	}
	if len(png) < 8 || string(png[:8]) != "\x89PNG\r\n\x1a\n" {
		fail(fmt.Errorf("%s is not a PNG file", os.Args[1]))
	}

	// ICO 可包含 PNG 图像。256x256 的图标用目录项中的 0 表示尺寸 256。
	ico := make([]byte, 22+len(png))
	binary.LittleEndian.PutUint16(ico[0:2], 0)
	binary.LittleEndian.PutUint16(ico[2:4], 1)
	binary.LittleEndian.PutUint16(ico[4:6], 1)
	ico[6] = 0
	ico[7] = 0
	ico[8] = 0
	ico[9] = 0
	binary.LittleEndian.PutUint16(ico[10:12], 1)
	binary.LittleEndian.PutUint16(ico[12:14], 32)
	binary.LittleEndian.PutUint32(ico[14:18], uint32(len(png)))
	binary.LittleEndian.PutUint32(ico[18:22], 22)
	copy(ico[22:], png)
	if err := os.WriteFile(os.Args[2], ico, 0o644); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
