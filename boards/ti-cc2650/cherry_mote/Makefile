# Makefile for building the tock kernel for the CC2650-CherryMote

# Provide your path to cc26x0 driverlib sources:
export DRIVERLIB_PATH ?= /home/xps15/Studia/Sem8/Tock/driverlib/cc26x0/driverlib
export NEWLIB_INC_PATH ?= /usr/arm-none-eabi/include

# Decide on kernel/apps flash and RAM division
export KERNEL_FLASH_LEN ?= 0x13000
export APPS_FLASH_BEG := $(KERNEL_FLASH_LEN)
export APPS_RAM_BEG := 0x20003000

TOCK_BOARD=ti-cc2650-cherry_mote
TARGET=thumbv7m-none-eabi
PLATFORM=ti-cc2650-cherry-mote

include ../../Makefile.common
include ../common/Makefile.common

export HENI_DEVICE ?= 677
HENI_PROGRAM := $(HENI) node program cherry -d dev:$(HENI_DEVICE)
HENI_SCP := $(HENI) supervisor scp -d dev:$(HENI_DEVICE)

### Flashing kernel only
.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).flash
	$(HENI_PROGRAM) $<

### Flashing kernel + an app
.PHONY: app-%-flash
app-%-flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.flash
	$(HENI_PROGRAM) $<

### SCP kernel + an app .bin
.PHONY: app-%-bin-scp
app-%-bin-scp: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.bin
	$(HENI_SCP) $< /tmp/node_program.bin
