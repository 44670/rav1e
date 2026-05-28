ifeq ($(strip $(DEVKITPRO)),)
$(error DEVKITPRO is not set)
endif
ifeq ($(strip $(DEVKITARM)),)
$(error DEVKITARM is not set)
endif

TOPDIR ?= $(CURDIR)
export TOPDIR
include $(DEVKITPRO)/devkitARM/3ds_rules

TARGET := o3yvbench
BUILD := build
SOURCES := source generated
INCLUDES := include generated

ARCH := -march=armv6k -mtune=mpcore -mfloat-abi=hard -mtp=soft
CFLAGS := -g -Wall -O3 -mword-relocations -ffunction-sections $(ARCH)
CFLAGS += $(INCLUDE) -D__3DS__
CXXFLAGS := $(CFLAGS) -fno-rtti -fno-exceptions
ASFLAGS := -g $(ARCH)
LDFLAGS = -specs=3dsx.specs -g $(ARCH) -Wl,-Map,$(notdir $*.map)

LIBS := $(RUST_LIB) -lctru -lm
LIBDIRS := $(CTRULIB)

ifeq ($(strip $(RUST_LIB)),)
$(error RUST_LIB must point to libminidecoder_3dsffi.a)
endif

ifneq ($(BUILD),$(notdir $(CURDIR)))

export OUTPUT := $(CURDIR)/$(BUILD)/$(TARGET)
export VPATH := $(foreach dir,$(SOURCES),$(CURDIR)/$(dir))
export DEPSDIR := $(CURDIR)/$(BUILD)

CFILES := $(foreach dir,$(SOURCES),$(notdir $(wildcard $(dir)/*.c)))
SFILES := $(foreach dir,$(SOURCES),$(notdir $(wildcard $(dir)/*.s)))
OFILES := $(CFILES:.c=.o) $(SFILES:.s=.o)

export INCLUDE := $(foreach dir,$(INCLUDES),-I$(CURDIR)/$(dir)) \
  $(foreach dir,$(LIBDIRS),-I$(dir)/include)
export LIBPATHS := $(foreach dir,$(LIBDIRS),-L$(dir)/lib)
export OFILES := $(OFILES)

.PHONY: all clean

all: $(BUILD)

$(BUILD):
	@[ -d $@ ] || mkdir -p $@
	@$(MAKE) --no-print-directory -C $(BUILD) -f $(CURDIR)/Makefile

clean:
	@echo clean ...
	@rm -fr $(BUILD)

else

.PHONY: all

all: $(OUTPUT).3dsx

$(OUTPUT).3dsx: $(OUTPUT).elf

$(OUTPUT).elf: $(OFILES)

%.elf:
	@echo linking $(notdir $@)
	@$(CC) $(LDFLAGS) $(OFILES) $(LIBPATHS) $(LIBS) -o $@

%.o: %.c
	@echo $(notdir $<)
	@$(CC) $(CFLAGS) -MMD -MP -MF $(DEPSDIR)/$*.d -c $< -o $@

%.o: %.s
	@echo $(notdir $<)
	@$(CC) $(ASFLAGS) -MMD -MP -MF $(DEPSDIR)/$*.d -c $< -o $@

-include $(DEPSDIR)/*.d

endif
