// The dough engine as one translation unit. No audio back end is selected:
// Rudel renders it into its own mixer (see lib.rs).
// SPDX-License-Identifier: AGPL-3.0-or-later
// Everything dough.h includes for a native build, first, so that the renames
// below reach only dough's own calls (MSVC's <math.h> declares `atof` too).
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// The browser runs dough as WebAssembly, built without a C library, so it
// parses every number in a message with dough's own `atof` and `atoi`
// (dough.h, the CLANGWASM branch). The C library's round some decimals
// differently, enough to move an event timed `time/0.03` by a sample. These
// are copies of dough's, used in their place, so a message means here exactly
// what it means in the browser.
static int dough_atoi(const char *str)
{
  int is_negative = 0;
  if (*str == '-')
  {
    is_negative = 1;
    ++str;
  }
  int acc = 0;
  while (*str)
  {
    acc *= 10;
    const int digit = (*str - '0');
    if ((digit < 0) || (9 < digit))
      return 0; // error
    acc += digit;
    ++str;
  }
  return is_negative ? -acc : acc;
}

static int dough_isdigit(int c)
{
  return ('0' <= c) && (c <= '9');
}

static double dough_atof(const char *str)
{
  double sign_multiplier = 1.0;
  if (*str == '-')
  {
    sign_multiplier = -1.0;
    ++str;
  }

  // parse integer part
  const char *integer_end = str;
  while (*integer_end && dough_isdigit(*integer_end))
  {
    ++integer_end;
  }
  double integer_part = 0.0;
  for (; str < integer_end; ++str)
  {
    integer_part *= 10.0;
    const int digit = (*str - '0');
    if ((digit < 0) || (9 < digit))
      break;
    integer_part += (double)digit;
  }

  char stop = *str;
  if (stop == 0)
  {
    return sign_multiplier * integer_part;
  }

  double significand = integer_part;
  if (stop == '.')
  {
    ++str;
    double fractional_multiplier = 1.0;
    while (dough_isdigit(*str))
    {
      fractional_multiplier *= 0.1;
      const int digit = (*str - '0');
      significand += fractional_multiplier * (double)digit;
      ++str;
    }
  }

  stop = *str;
  if (stop == 0)
  {
    return sign_multiplier * significand;
  }

  int exponent = 0;
  if (stop == 'e' || stop == 'E')
  {
    exponent = dough_atoi(str + 1);
  }

  double value = significand;
  while (exponent > 0)
  {
    value *= 10.0;
    --exponent;
  }
  while (exponent < 0)
  {
    value *= 0.1;
    ++exponent;
  }
  return sign_multiplier * value;
}

// The same for the three maths functions the WebAssembly build defines itself
// that a C library may not match: its `roundf` rounds halves up, not away
// from zero, and its min/max are plain comparisons. Every other function
// dough calls is either its own (the `our_*` approximations, used in both
// builds) or one IEEE requires to be exact (`floor`, `sqrt`, `fabs`).
static float dough_roundf(float x)
{
  return floorf(x + 0.5f);
}

static float dough_fminf(float x, float y)
{
  return x < y ? x : y;
}

static float dough_fmaxf(float x, float y)
{
  return x > y ? x : y;
}

// The headers are already in, so dough.h's own includes change nothing and
// these names only reach dough's calls.
#define atof dough_atof
#define atoi dough_atoi
#define roundf dough_roundf
#define fminf dough_fminf
#define fmaxf dough_fmaxf

#define DOUGH_IMPLEMENTATION
#include "dough.h"
