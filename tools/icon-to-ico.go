package main

import (
	"encoding/binary"
	"fmt"
	"os"
	"sort"
)

func main() {
	if len(os.Args) < 3 {
		fmt.Fprintln(os.Stderr, "usage: go run tools/icon-to-ico.go input.png output.ico [additional-input.png ...]")
		os.Exit(2)
	}
	type imageData struct {
		width  int
		height int
		data   []byte
	}
	images := make([]imageData, 0, len(os.Args)-2)
	seen := make(map[int]struct{})
	inputs := make([]string, 0, len(os.Args)-2)
	inputs = append(inputs, os.Args[1])
	inputs = append(inputs, os.Args[3:]...)
	for _, input := range inputs {
		png, err := os.ReadFile(input)
		if err != nil {
			fail(err)
		}
		width, height, err := pngDimensions(png)
		if err != nil {
			fail(fmt.Errorf("%s: %w", input, err))
		}
		if width != height || width == 0 || width > 256 {
			fail(fmt.Errorf("%s has unsupported dimensions %dx%d", input, width, height))
		}
		if _, ok := seen[int(width)]; ok {
			fail(fmt.Errorf("duplicate icon size %d in %s", width, input))
		}
		seen[int(width)] = struct{}{}
		images = append(images, imageData{width: int(width), height: int(height), data: png})
	}
	if len(images) == 0 {
		fail(fmt.Errorf("no PNG inputs were provided"))
	}
	sort.Slice(images, func(i, j int) bool { return images[i].width < images[j].width })

	entrySize := 16
	headerSize := 6
	dataOffset := headerSize + entrySize*len(images)
	dataSize := 0
	for _, image := range images {
		dataSize += len(image.data)
	}
	ico := make([]byte, dataOffset+dataSize)
	binary.LittleEndian.PutUint16(ico[0:2], 0)
	binary.LittleEndian.PutUint16(ico[2:4], 1)
	binary.LittleEndian.PutUint16(ico[4:6], uint16(len(images)))
	dataCursor := dataOffset
	for index, image := range images {
		entry := headerSize + index*entrySize
		if image.width == 256 {
			ico[entry] = 0
			ico[entry+1] = 0
		} else {
			ico[entry] = byte(image.width)
			ico[entry+1] = byte(image.height)
		}
		ico[entry+2] = 0
		ico[entry+3] = 0
		binary.LittleEndian.PutUint16(ico[entry+4:entry+6], 1)
		binary.LittleEndian.PutUint16(ico[entry+6:entry+8], 32)
		binary.LittleEndian.PutUint32(ico[entry+8:entry+12], uint32(len(image.data)))
		binary.LittleEndian.PutUint32(ico[entry+12:entry+16], uint32(dataCursor))
		copy(ico[dataCursor:], image.data)
		dataCursor += len(image.data)
	}
	if err := os.WriteFile(os.Args[2], ico, 0o644); err != nil {
		fail(err)
	}
}

func pngDimensions(data []byte) (uint32, uint32, error) {
	if len(data) < 24 || string(data[:8]) != "\x89PNG\r\n\x1a\n" {
		return 0, 0, fmt.Errorf("not a PNG file")
	}
	if string(data[12:16]) != "IHDR" {
		return 0, 0, fmt.Errorf("PNG is missing IHDR")
	}
	return binary.BigEndian.Uint32(data[16:20]), binary.BigEndian.Uint32(data[20:24]), nil
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
