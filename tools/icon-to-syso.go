package main

import (
	"encoding/binary"
	"fmt"
	"os"
	"sort"
)

const (
	coffMachineAMD64           = 0x8664
	coffSectionHeaderSize      = 40
	coffFileHeaderSize         = 20
	coffSymbolSize             = 18
	coffRelocationSize         = 10
	imageRelAMD64Addr32NB      = 0x0003
	imageScnCntInitializedData = 0x00000040
	imageScnMemRead            = 0x40000000
	imageResourceTypeIcon      = 3
	imageResourceTypeGroupIcon = 14
)

type icoImage struct {
	width      int
	height     int
	colorCount byte
	planes     uint16
	bitCount   uint16
	data       []byte
}

type resourceNode struct {
	id       uint32
	children []*resourceNode
	dataID   int
}

type resourceRelocation struct {
	offset uint32
	dataID int
}

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: go run tools/icon-to-syso.go input.ico output.syso")
		os.Exit(2)
	}
	ico, err := readICO(os.Args[1])
	if err != nil {
		fail(err)
	}
	object, err := buildResourceObject(ico)
	if err != nil {
		fail(err)
	}
	if err := os.WriteFile(os.Args[2], object, 0o644); err != nil {
		fail(err)
	}
}

func readICO(path string) ([]icoImage, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if len(data) < 6 || binary.LittleEndian.Uint16(data[0:2]) != 0 || binary.LittleEndian.Uint16(data[2:4]) != 1 {
		return nil, fmt.Errorf("%s is not an ICO file", path)
	}
	count := int(binary.LittleEndian.Uint16(data[4:6]))
	if count == 0 || len(data) < 6+16*count {
		return nil, fmt.Errorf("%s has no complete ICO directory", path)
	}
	images := make([]icoImage, 0, count)
	for i := 0; i < count; i++ {
		entry := 6 + 16*i
		width, height := int(data[entry]), int(data[entry+1])
		if width == 0 {
			width = 256
		}
		if height == 0 {
			height = 256
		}
		bytesInRes := int(binary.LittleEndian.Uint32(data[entry+8 : entry+12]))
		offset := int(binary.LittleEndian.Uint32(data[entry+12 : entry+16]))
		if bytesInRes <= 0 || offset < 0 || offset > len(data) || bytesInRes > len(data)-offset {
			return nil, fmt.Errorf("%s has an invalid ICO image entry %d", path, i)
		}
		images = append(images, icoImage{
			width:      width,
			height:     height,
			colorCount: data[entry+2],
			planes:     binary.LittleEndian.Uint16(data[entry+4 : entry+6]),
			bitCount:   binary.LittleEndian.Uint16(data[entry+6 : entry+8]),
			data:       append([]byte(nil), data[offset:offset+bytesInRes]...),
		})
	}
	sort.Slice(images, func(i, j int) bool {
		if images[i].width != images[j].width {
			return images[i].width < images[j].width
		}
		return len(images[i].data) < len(images[j].data)
	})
	return images, nil
}

func buildResourceObject(images []icoImage) ([]byte, error) {
	if len(images) == 0 || len(images) > 255 {
		return nil, fmt.Errorf("ICO must contain between 1 and 255 images")
	}
	groupData := makeGroupIconData(images)
	resourceData := make([][]byte, 1, len(images)+1)
	resourceData[0] = groupData
	for _, image := range images {
		resourceData = append(resourceData, image.data)
	}

	groupLeaf := &resourceNode{id: 1033, dataID: 0}
	groupName := &resourceNode{id: 1, dataID: -1, children: []*resourceNode{groupLeaf}}
	groupType := &resourceNode{id: imageResourceTypeGroupIcon, dataID: -1, children: []*resourceNode{groupName}}
	iconChildren := make([]*resourceNode, 0, len(images))
	for i := range images {
		lang := &resourceNode{id: 1033, dataID: i + 1}
		iconChildren = append(iconChildren, &resourceNode{id: uint32(i + 1), dataID: -1, children: []*resourceNode{lang}})
	}
	iconType := &resourceNode{id: imageResourceTypeIcon, dataID: -1, children: iconChildren}
	// PE resource IDs must be stored in ascending order for Explorer and the
	// Windows resource APIs to resolve every type reliably.
	root := &resourceNode{id: 0, dataID: -1, children: []*resourceNode{iconType, groupType}}

	var directory []byte
	relocations := make([]resourceRelocation, 0, len(resourceData))
	var encode func(*resourceNode) uint32
	encode = func(node *resourceNode) uint32 {
		if node.dataID >= 0 {
			offset := uint32(len(directory))
			directory = append(directory, make([]byte, 16)...)
			// IMAGE_RESOURCE_DATA_ENTRY stores both the RVA (relocated by the
			// linker) and the payload size. Leaving Size at zero makes Windows
			// report a present-but-empty RT_ICON/RT_GROUP_ICON resource.
			binary.LittleEndian.PutUint32(directory[offset+4:offset+8], uint32(len(resourceData[node.dataID])))
			relocations = append(relocations, resourceRelocation{offset: offset, dataID: node.dataID})
			return offset
		}
		offset := uint32(len(directory))
		// IMAGE_RESOURCE_DIRECTORY_ENTRY is eight bytes; only leaf data entries
		// use the 16-byte IMAGE_RESOURCE_DATA_ENTRY layout.
		directory = append(directory, make([]byte, 16+8*len(node.children))...)
		childOffsets := make([]uint32, len(node.children))
		for i, child := range node.children {
			childOffsets[i] = encode(child)
		}
		// Resource directories store named and numeric entries separately.
		// Every icon node here uses numeric IDs, so the count belongs in
		// NumberOfIdEntries (offset + 14), not NumberOfNamedEntries.
		binary.LittleEndian.PutUint16(directory[offset+14:offset+16], uint16(len(node.children)))
		for i, child := range node.children {
			entry := int(offset) + 16 + 8*i
			binary.LittleEndian.PutUint32(directory[entry:entry+4], child.id)
			childOffset := childOffsets[i]
			if child.dataID < 0 {
				childOffset |= 0x80000000
			}
			binary.LittleEndian.PutUint32(directory[entry+4:entry+8], childOffset)
		}
		return offset
	}
	if rootOffset := encode(root); rootOffset != 0 {
		return nil, fmt.Errorf("resource root did not start at offset zero")
	}
	if len(directory) == 0 || len(directory)%4 != 0 {
		return nil, fmt.Errorf("resource directory alignment error")
	}

	var rawData []byte
	dataOffsets := make([]uint32, len(resourceData))
	for i, data := range resourceData {
		for len(rawData)%4 != 0 {
			rawData = append(rawData, 0)
		}
		dataOffsets[i] = uint32(len(rawData))
		rawData = append(rawData, data...)
	}
	return writeCOFF(directory, rawData, dataOffsets, relocations)
}

func makeGroupIconData(images []icoImage) []byte {
	data := make([]byte, 6+14*len(images))
	binary.LittleEndian.PutUint16(data[2:4], 1)
	binary.LittleEndian.PutUint16(data[4:6], uint16(len(images)))
	for i, image := range images {
		offset := 6 + 14*i
		if image.width == 256 {
			data[offset] = 0
		} else {
			data[offset] = byte(image.width)
		}
		if image.height == 256 {
			data[offset+1] = 0
		} else {
			data[offset+1] = byte(image.height)
		}
		data[offset+2] = image.colorCount
		binary.LittleEndian.PutUint16(data[offset+4:offset+6], image.planes)
		binary.LittleEndian.PutUint16(data[offset+6:offset+8], image.bitCount)
		binary.LittleEndian.PutUint32(data[offset+8:offset+12], uint32(len(image.data)))
		binary.LittleEndian.PutUint16(data[offset+12:offset+14], uint16(i+1))
	}
	return data
}

func writeCOFF(directory, rawData []byte, dataOffsets []uint32, relocations []resourceRelocation) ([]byte, error) {
	const sectionCharacteristics = imageScnCntInitializedData | imageScnMemRead
	section1Offset := uint32(coffFileHeaderSize + 2*coffSectionHeaderSize)
	section2Offset := align4(section1Offset + uint32(len(directory)))
	relocationOffset := align4(section2Offset + uint32(len(rawData)))
	symbolOffset := align4(relocationOffset + uint32(coffRelocationSize*len(relocations)))
	numberSymbols := uint32(4 + len(dataOffsets))
	total := symbolOffset + numberSymbols*coffSymbolSize + 4
	result := make([]byte, total)

	binary.LittleEndian.PutUint16(result[0:2], coffMachineAMD64)
	binary.LittleEndian.PutUint16(result[2:4], 2)
	binary.LittleEndian.PutUint32(result[8:12], symbolOffset)
	binary.LittleEndian.PutUint32(result[12:16], numberSymbols)

	writeSectionHeader(result[coffFileHeaderSize:], ".rsrc$01", uint32(len(directory)), section1Offset, relocationOffset, uint16(len(relocations)), sectionCharacteristics)
	writeSectionHeader(result[coffFileHeaderSize+coffSectionHeaderSize:], ".rsrc$02", uint32(len(rawData)), section2Offset, 0, 0, sectionCharacteristics)
	copy(result[section1Offset:], directory)
	copy(result[section2Offset:], rawData)

	for i, relocation := range relocations {
		offset := int(relocationOffset) + coffRelocationSize*i
		binary.LittleEndian.PutUint32(result[offset:offset+4], relocation.offset)
		binary.LittleEndian.PutUint32(result[offset+4:offset+8], uint32(4+relocation.dataID))
		binary.LittleEndian.PutUint16(result[offset+8:offset+10], imageRelAMD64Addr32NB)
	}

	symbols := int(symbolOffset)
	writeSectionSymbol(result[symbols:], ".rsrc$01", 1, uint32(len(directory)), uint16(len(relocations)))
	writeSectionSymbol(result[symbols+2*coffSymbolSize:], ".rsrc$02", 2, uint32(len(rawData)), 0)
	for i, dataOffset := range dataOffsets {
		writeDataSymbol(result[symbols+(4+i)*coffSymbolSize:], fmt.Sprintf("$R%06d", i), dataOffset, 2)
	}
	return result, nil
}

func writeSectionHeader(dst []byte, name string, size, rawOffset, relocationOffset uint32, relocationCount uint16, characteristics uint32) {
	copy(dst[:8], []byte(name))
	binary.LittleEndian.PutUint32(dst[16:20], size)
	binary.LittleEndian.PutUint32(dst[20:24], rawOffset)
	binary.LittleEndian.PutUint32(dst[24:28], relocationOffset)
	binary.LittleEndian.PutUint16(dst[32:34], relocationCount)
	binary.LittleEndian.PutUint32(dst[36:40], characteristics)
}

func writeSectionSymbol(dst []byte, name string, section uint16, size uint32, relocationCount uint16) {
	writeSymbol(dst, name, 0, section, 0, 3, 1)
	aux := dst[18:36]
	binary.LittleEndian.PutUint32(aux[0:4], size)
	binary.LittleEndian.PutUint16(aux[4:6], relocationCount)
}

func writeDataSymbol(dst []byte, name string, value uint32, section uint16) {
	writeSymbol(dst, name, value, section, 0, 3, 0)
}

func writeSymbol(dst []byte, name string, value uint32, section uint16, typ uint16, storageClass byte, aux byte) {
	copy(dst[:8], []byte(name))
	binary.LittleEndian.PutUint32(dst[8:12], value)
	binary.LittleEndian.PutUint16(dst[12:14], section)
	binary.LittleEndian.PutUint16(dst[14:16], typ)
	dst[16] = storageClass
	dst[17] = aux
}

func align4(value uint32) uint32 {
	return (value + 3) &^ 3
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
