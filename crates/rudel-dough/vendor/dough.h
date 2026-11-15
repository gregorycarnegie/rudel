// build for native:
// dough.c
// license: AGPL-3.0
// This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU Affero General Public License for more details. You should have received a copy of the GNU Affero General Public License along with this program.  If not, see <https://www.gnu.org/licenses/>.

void dough_init(int sample_rate, int max_voices, int max_orbits, int max_delay_time, int max_pcm, int max_events, int max_files);
int dough_start_audio(void);
void dough_stop_audio(void);
void dough_evaluate(const char *msg);

#ifdef DOUGH_IMPLEMENTATION

#include <stdbool.h>

#ifndef CLANGWASM

#include <math.h> // <- this adds 4KB :'(
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>

#else // CLANGWASM

#include <stddef.h>
#include <stdint.h>

#define NAN __builtin_nanf("")
#define NO_RETURN __attribute__((noreturn))

extern void js_init(const char *); // implemented in JS

extern void js_panic(const char *); // implemented in JS
NO_RETURN static void PANIC(const char *message)
{
  js_panic(message);
  __builtin_trap();
}

#define STRHACK2(s) #s // macro hacks for making __LINE__ into a string
#define STRHACK(s) STRHACK2(s)
#define assert(p)                                                      \
  if (!(p))                                                            \
  {                                                                    \
    PANIC("assert(" #p ") failed at " __FILE__ ":" STRHACK(__LINE__)); \
  }

static size_t strlen(const char *s)
{
  size_t num = 0;
  while (*s)
  {
    ++s;
    ++num;
  }
  return num;
}

static int strcmp(const char *s1, const char *s2)
{
  while (*s1 && *s2 && (*s1 == *s2))
  {
    ++s1;
    ++s2;
  }
  return (int)(*s1) - (int)(*s2);
}

char *strcat(char *dst, const char *src)
{
  char *p = dst;
  while (*p)
    p++;
  while ((*p++ = *src++))
    ;
  return dst;
}

static char *strncpy(char *dst, const char *src, size_t max)
{
  size_t remaining = max;
  char *dstp = dst;
  while (*src && (remaining > 0))
  {
    *(dstp++) = *(src++);
    --remaining;
  }
  *dstp = 0;
  return dst;
}

// needs -mbulk-memory to not explode
void *memcpy(void *dest, const void *src, size_t count)
{
  return __builtin_memcpy(dest, src, count);
}

static char *strtok_next_pos, *strtok_end;
static char *strtok(char *str, const char *sep)
{
  const int num_sep = strlen(sep);
  if (str != NULL)
  {
    strtok_next_pos = str;
    strtok_end = str + strlen(str); // before we begin NUL'ing the string...
  }
  assert((strtok_next_pos != NULL) && "invalid strtok()-usage");

  char *p = strtok_next_pos;
  if (p >= strtok_end)
  {
    return NULL;
  }
  char *p0 = p;
  for (; p < strtok_end; ++p)
  {
    char c = *p;
    int found_sep = 0;
    for (int i = 0; i < num_sep; ++i)
    {
      if (c == sep[i])
      {
        found_sep = 1;
        break;
      }
    }
    if (found_sep)
      break;
  }
  *p = 0; // NUL-terminate string
  strtok_next_pos = p + 1;
  return p0;
}

static int atoi(const char *str)
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

// positive integer to ascii
char *itoa_u(int v, char *out)
{
  char buf[20];
  int n = 0;

  if (v == 0)
    buf[n++] = '0';
  else
    while (v)
    {
      buf[n++] = '0' + (v % 10);
      v /= 10;
    }

  for (int i = 0; i < n; ++i)
    out[i] = buf[n - 1 - i];

  out[n] = '\0';
  return out;
}

static inline int isdigit(int c)
{
  return ('0' <= c) && (c <= '9');
}

static double atof(const char *str)
{
  double sign_multiplier = 1.0;
  if (*str == '-')
  {
    sign_multiplier = -1.0;
    ++str;
  }

  // parse integer part
  const char *integer_end = str;
  while (*integer_end && isdigit(*integer_end))
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
    while (isdigit(*str))
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
    exponent = atoi(str + 1);
  }

  double value = significand;
  // not too efficient but avoids powf()-dependency
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
  assert(exponent == 0);
  return sign_multiplier * value;
}

extern unsigned char __heap_base;
static size_t heap_bytes_allocated;
void *malloc(size_t size)
{
  const int align = (1 << 4);               // 1<<4=16B is the largest WASM alignment (for 128-bit SIMD values)?
  size = (size + align - 1) & ~(align - 1); // round size up to alignment
  void *base = (void *)&__heap_base + heap_bytes_allocated;
  heap_bytes_allocated += size;
  assert((heap_bytes_allocated <= CLANGWASM_MAXMEM) && "out of memory!");
  return base;
}

//////////////////
// math

#define M_PI (3.141592653589793)

static inline float floorf(float x) { return __builtin_floorf(x); }
static inline double floor(double x) { return __builtin_floor(x); }
static inline float fabsf(float x) { return __builtin_fabsf(x); }
static inline float sqrtf(float x) { return __builtin_sqrtf(x); }
#define isnan(x) __builtin_isnan(x) // already a float/double macro so don't make it a function

static inline float roundf(float x)
{
  return floorf(x + 0.5f);
}

static inline float fminf(float x, float y)
{
  return x < y ? x : y;
}

static inline float fmaxf(float x, float y)
{
  return x > y ? x : y;
}

#endif // CLANGWASM

#define fPI ((float)M_PI) // actually speeds things up (implicit type conversion sucks)

union x32
{
  float f;
  int i;
};

#define F32_BIAS (0x7f)
#define F32_EXP_SHIFT (23)
#define F32_EXP_BITS (8)
#define F32_EXP_MASK (((1 << F32_EXP_BITS) - 1) << F32_EXP_SHIFT)

// note(aks): log2f impl stolen/adapted from:
//   http://www.machinedlearnings.com/2011/06/fast-approximate-logarithm-exponential.html
// I intend to replace this with a clean room representation, but I'm still
// going to "steal" a cool main insight: if you bitcast a float to int, and
// convert it back to float, and divide it by (2**F32_EXP_SHIFT) it's actually
// not a bad log2f approximation. it works because the exponent is already the
// integer part of the result we want, and the significant acts as a "linear
// scalar" between subsequent exponent-values. the only "problem" is
// that the significand is linear, and we need something to "bend" it.
float our_log2f(float x)
{
  union x32 xx0;
  xx0.f = x;

  union x32 xx1;
  xx1.i = (xx0.i & ((1 << F32_EXP_SHIFT) - 1)) | ((F32_BIAS - 1) << F32_EXP_SHIFT);

  float y = xx0.i;
  y *= 1.0f / (float)(1 << 23);
  return y - 124.22544637f - 1.498030302f * xx1.f - 1.72587999f / (0.3520887068f + xx1.f);
}

static inline float our_exp2f(float x)
{
  // adapted from doc/blog_fnapprox_03.html
  const float xf = floorf(x);
  union x32 xx;
  xx.i = (127 + ((int)xf)) << 23;
  const float ystep = xx.f;
  const float x1 = x - xf;
  const float xt = x1 - 0.5f;
  const float c0 = 1.4142135623730951f;
  const float c1 = 0.9802581434685472f;
  const float c2 = 0.3397315841830749f;
  const float c3 = 0.07849466324122069f;
  const float c4 = 0.013602088628663625f;
  const float ytaylor = c0 + xt * (c1 + xt * (c2 + xt * (c3 + xt * (c4))));
  const float m0 = 0.9999443187818418f;
  const float m1 = 1.0000312541780023f;
  return ystep * ytaylor * (m0 + (m1 - m0) * x1);
}

static inline float our_powf(float x, float y)
{
  if (x < 0.0f)
    return NAN; // log(x) is undefined for x<=0
  if (x == 0.0f)
    return 0.0f;
  if (y == 0.0f)
    return x >= 0.0f ? 1.0f : -1.0f;
  return our_exp2f(y * our_log2f(x)); // general case
}

static inline float our_expm1f(float x)
{
  const float log2_of_e = 1.4426950408889634f;
  return our_exp2f(x * log2_of_e) - 1.0f;
}

static inline float pow10(float x)
{
  const float log2_of_10 = 3.321928094887362f;
  return our_exp2f(x * log2_of_10);
}

// wraps x into [-pi;pi] range
static inline float modpi(float x)
{
  x += fPI;
  x *= (0.5f / fPI);
  x -= floorf(x);
  x *= (2.0f * fPI);
  x -= fPI;
  return x;
}

// "sine" made from a parabola
static inline float par_sinf(float x)
{
  x = modpi(x);
  return 0.40528473456935094f * x * (fPI - fabsf(x));
}

static inline float par_cosf(float x)
{
  return par_sinf(x + (0.5f * fPI));
}

// source: https://en.wikipedia.org/wiki/Pad%C3%A9_approximant#Examples
// could probably lose an order or so, but these rational functions aren't easy
// to "truncate" like Taylor series (by simply removing higher orders), so I'd
// have to actually understand how to make these
static inline float our_sinf(float x)
{
  x = (4.0f * fabsf(x * (.5f / fPI) - floorf(x * (.5f / fPI) + (3.0f / 4.0f)) + (1.0f / 4.0f)) - 1.0f) * (fPI / 2.0f);
  const float c0 = 1.0f;
  const float c1 = 1.0f;
  const float c2 = (445.0f / 12122.0f);
  const float c3 = -(2363.0f / 18183.0f);
  const float c4 = (601.0f / 872784.0f);
  const float c5 = (12671.0f / 4363920.0f);
  const float c6 = (121.0f / 16662240.0f);
  // approximation is:
  //          c5*x^5 - c3*x^3 + x
  // y = ----------------------------
  //     1 + c2*x^2 + c4*x^4 + c6*x^6
  // using Horner's method
  //   c0 + c1*x^1 + c2*x^2 + ...
  // = c0 + x*(c1 + x*(c2 + ... ))
  // since this approximation "skips" orders (x, x^3, x^5,..) we can just
  // "collapse" every second order in Horner's method:
  //   x*(c1 + x*(c2 + x*(c3 + x*(c4 + x*(c5 + ... ))))), for c2=c4=0:
  // = x*(c1 + x*(0  + x*(c3 + x*(0  + x*(c5 + ... )))))
  // = x*(c1       + x*x*(c3       + x*x*(c5 + ... )))
  const float xx = x * x;
  const float num = x * (c1 + xx * (c3 + xx * (c5)));
  const float denom = c0 + xx * (c2 + xx * (c4 + xx * (c6)));
  return num / denom;
}

#if 0
static inline float our_cosf(float x)
{
  return our_sinf(x + (0.5f*fPI));
}
#endif

// SAMPLE File
typedef struct
{
  float *pcm;
  int channels;
  int frames;
  float freq;
} FileSource;

// Note [Dough Engine]
// ~~~~~~~~~~~~~~~~~~~
//
// The engine defines how dough operates, in particular:
// - the desired sample rate (sr/isr/lag_unit)
// - the number of voices/orbits/scheduled events/delay length
//
// When initialized, a continuous memory chunk is allocated and stored in the engine.memory:
//   | voices | orbits | delay lines | scheduler events | pcm data | framebuffer |
//
// A global 'engine' instance is available so that 'play_event' and 'dsp' can be called directly, without passing a reference.
typedef struct
{
  float sr;
  float isr;
  int lag_unit;
  int active_voices;
  uint64_t voice_counter; // increments each time a voice is played
  int max_voices;
  int max_orbits;
  int max_events;
  int max_delay_samples;
  int framebuffer_length;
  float context_offset;
  char *memory;
  float *pcm;
  int pcm_offset;
  int max_pcm;
  float *framebuffer;
  FileSource *files;
  int max_files;
} Engine;
Engine engine = {};

#define BLOCK_SIZE 128
#define CHANNELS 2 // could this be dynamic?
#define MAX(x, y) (((x) > (y)) ? (x) : (y))
#define MIN(x, y) (((x) < (y)) ? (x) : (y))

#define GAIN_SINE 0.2
#define GAIN_TRI 0.2
#define GAIN_SAW 0.2
#define GAIN_PULSE 0.2
#define GAIN_WHITE 0.2
#define GAIN_PINK 0.2
#define GAIN_BROWN 0.2
#define GAIN_MIX 0.5

unsigned int seed = 123456789;

unsigned int lcg_rand()
{
  seed = seed * 1103515245 + 12345;
  return (seed >> 16) & 0x7fff;
}

float random_float()
{
  return ((float)lcg_rand() / 32767.0f) /*  * 2.0f - 1.0f */;
}

float lerp(float x, float y0, float y1, float exponent)
{
  if (x <= 0)
    return y0;
  if (x >= 1)
    return y1;
  float curvedX;

  // thanks jade
  if (exponent == 0)
    curvedX = x; // linear
  else if (exponent > 0)
    curvedX = our_powf(x, exponent); // ease-in
  else
    curvedX = 1 - our_powf(1 - x, -exponent); // ease-out

  return y0 + (y1 - y0) * curvedX;
}

float clamp(float value, float min, float max)
{
  if (value < min)
    return min;
  if (value > max)
    return max;
  return value;
}

void nan_fallback(float *value, float fallback)
{
  if (isnan(*value))
    *value = fallback;
}

float ftz(float x, float limit)
{
  if (x < limit && x > -limit)
    return 0.0;
  return x;
}

float polyBlep(float t, float dt)
{
  // 0 <= t < 1
  if (t < dt)
  {
    t /= dt;
    // 2 * (t - t^2/2 - 0.5)
    return t + t - t * t - 1;
  }
  // -1 < t < 0
  if (t > 1 - dt)
  {
    t = (t - 1) / dt;
    // 2 * (t^2/2 + t + 0.5)
    return t * t + t + t + 1;
  }
  // 0 otherwise
  return 0;
}

// returns true if phase was wrapped this sample
bool update_phasor(float *phase, float freq)
{
  *phase += freq * engine.isr;
  // *phase = fmodf(*phase, 1.0f);
  if (*phase >= 1.0)
  {
    *phase -= 1.0; // Keeping phase in [0, 1)
    return true;
  }
  else if (*phase < 0)
  {
    *phase += 1.0; // thanks aria (https://codeberg.org/uzu/strudel/pulls/1495)
    return true;
  }
  return false;
}

typedef struct Phasor
{
  float phase;
} Phasor;

void Phasor_init(Phasor *self) { self->phase = 0; }

float TriOsc_update(Phasor *phasor, float freq)
{
  float s = phasor->phase < 0.5 ? 4 * phasor->phase - 1 : 3 - 4 * phasor->phase;
  update_phasor(&phasor->phase, freq);
  return s;
}

float SineOsc_update(Phasor *phasor, float freq)
{
  float s = our_sinf(phasor->phase * 2.0 * fPI);
  update_phasor(&phasor->phase, freq);
  return s;
}

float ZawOsc_update(Phasor *phasor, float freq)
{
  float s = (phasor->phase) * 2 - 1;
  update_phasor(&phasor->phase, freq);
  return s;
}

float SawOsc_update(Phasor *phasor, float freq)
{
  float p = polyBlep(phasor->phase, freq * engine.isr);
  float s = (phasor->phase) * 2 - 1 - p;
  update_phasor(&phasor->phase, freq);
  return s;
}

float PulseOsc_update(Phasor *phasor, float freq, float pw)
{
  float dt = freq * engine.isr;
  float phi = phasor->phase + pw;
  if (phi >= 1)
    phi -= 1;
  float p1 = polyBlep(phi, dt);
  float p2 = polyBlep(phasor->phase, dt);
  // idea: a pulse wave is a saw minus another saw offset by pw
  float pulse = 2 * (phasor->phase - phi) - p2 + p1;

  update_phasor(&phasor->phase, freq);
  return pulse + pw * 2 - 1;
}

float PulzeOsc_update(Phasor *phasor, float freq, float duty)
{
  float s = phasor->phase < duty ? 1 : -1;
  update_phasor(&phasor->phase, freq);
  return s;
}

bool FileSource_update(Phasor *phasor, float freq, FileSource *file_source, float begin, float end, float *out)
{
  int begin_frame = (int)(begin * file_source->frames);
  int end_frame = (int)(end * file_source->frames);
  int slice_frames = end_frame - begin_frame;
  // silence if end <= begin
  if (slice_frames <= 0)
  {
    for (int c = 0; c < CHANNELS; c++)
      out[c] = 0;
    return false;
  }
  int current_frame = begin_frame + (int)floor(phasor->phase * slice_frames);
  if (current_frame >= end_frame)
    current_frame = end_frame - 1;

  for (int c = 0; c < CHANNELS; c++)
  {
    int channel = c % file_source->channels;
    float s = file_source->pcm[current_frame * file_source->channels + channel];
    out[c] = s;
  }

  return update_phasor(&phasor->phase, freq);
}

// NOISE

float NoiseOsc_update() { return random_float() * 2 - 1; }

typedef struct PinkNoise
{
  float b0;
  float b1;
  float b2;
  float b3;
  float b4;
  float b5;
  float b6;
} PinkNoise;

void PinkNoise_init(PinkNoise *self)
{

  self->b0 = 0;
  self->b1 = 0;
  self->b2 = 0;
  self->b3 = 0;
  self->b4 = 0;
  self->b5 = 0;
  self->b6 = 0;
}

float PinkNoise_update(PinkNoise *self)
{

  float white = random_float() * 2 - 1;

  self->b0 = 0.99886 * self->b0 + white * 0.0555179;
  self->b1 = 0.99332 * self->b1 + white * 0.0750759;
  self->b2 = 0.969 * self->b2 + white * 0.153852;
  self->b3 = 0.8665 * self->b3 + white * 0.3104856;
  self->b4 = 0.55 * self->b4 + white * 0.5329522;
  self->b5 = -0.7616 * self->b5 - white * 0.016898;
  float pink = self->b0 + self->b1 + self->b2 + self->b3 + self->b4 + self->b5 +
               self->b6 + white * 0.5362;
  self->b6 = white * 0.115926;
  return pink * 0.11;
}

typedef struct BrownNoise
{
  float out;
} BrownNoise;

void BrownNoise_init(BrownNoise *self) { self->out = 0; }

float BrownNoise_update(BrownNoise *self)
{
  float white = random_float() * 2.0 - 1.0;
  self->out = (self->out + 0.02 * white) / 1.02;
  return self->out;
}

typedef enum
{
  ADSR_OFF,
  ADSR_ATTACK,
  ADSR_DECAY,
  ADSR_SUSTAIN,
  ADSR_RELEASE
} ADSRState;

typedef struct ADSRNode
{
  ADSRState state;
  float startTime;
  float startVal;
  float attackCurve;
  float decayCurve;
} ADSRNode;

void ADSRNode_init(ADSRNode *env)
{
  env->state = ADSR_OFF;
  env->startTime = 0.0;
  env->startVal = 0.0;
  env->attackCurve = 2.0;
  env->decayCurve = 2.0;
}

// TODO: test this with all the other envelopes
void init_envelope(float *env, float *att, float *dec, float *sus, float *rel, bool *is_active)
{
  if (isnan(*env) && isnan(*att) && isnan(*dec) && isnan(*sus) && isnan(*rel))
  {
    *is_active = false; // no param set -> disable envelope
    return;
  }
  *is_active = true;
  // float envmin = 0.01;
  float envmax = 1.0;

  // sus is NAN -> set sensible value based on attacl/decay
  if (isnan(*sus) && !isnan(*att) && isnan(*dec))
    *sus = 1.0; // A envelope
  else if (isnan(*sus) && isnan(*att) && !isnan(*dec))
    *sus = 0.0; // D envelope
  else if (isnan(*sus) && !isnan(*att) && !isnan(*dec))
    *sus = 0.0; // AD envelope
  else if (!isnan(*sus))
    *sus = fminf(*sus, envmax); // (A|D)S envelope

  nan_fallback(env, 1.0);
  nan_fallback(att, 0.001); // prevent clicks
  nan_fallback(dec, 0.0);
  nan_fallback(sus, 1.0);
  nan_fallback(rel, 0.005); // prevent clicks
}

float ADSRNode_update(ADSRNode *env, float curTime, float gate,
                      float attack, float decay, float susVal,
                      float release)
{
  switch (env->state)
  {
  case ADSR_OFF:
    if (gate > 0)
    {
      env->state = ADSR_ATTACK;
      env->startTime = curTime;
      env->startVal = 0.0;
    }
    return 0.0;

  case ADSR_ATTACK:
  {
    float time = curTime - env->startTime;
    if (time > attack)
    {
      env->state = ADSR_DECAY;
      env->startTime = curTime;
      return 1.0;
    }
    return lerp(time / attack, env->startVal, 1.0, env->attackCurve);
  }

  case ADSR_DECAY:
  {
    float time = curTime - env->startTime;
    float curVal = lerp(time / decay, 1.0, susVal, -env->decayCurve);

    if (gate <= 0)
    {
      env->state = ADSR_RELEASE;
      env->startTime = curTime;
      env->startVal = curVal;
      return curVal;
    }

    if (time > decay)
    {
      env->state = ADSR_SUSTAIN;
      env->startTime = curTime;
      return susVal;
    }

    return curVal;
  }

  case ADSR_SUSTAIN:
    if (gate <= 0)
    {
      env->state = ADSR_RELEASE;
      env->startTime = curTime;
      env->startVal = susVal;
    }
    return susVal;

  case ADSR_RELEASE:
  {
    float time = curTime - env->startTime;
    if (time > release)
    {
      env->state = ADSR_OFF;
      return 0.0;
    }

    float curVal = lerp(time / release, env->startVal, 0.0, -env->decayCurve);
    if (gate > 0)
    {
      env->state = ADSR_ATTACK;
      env->startTime = curTime;
      env->startVal = curVal;
    }
    return curVal;
  }
  }
  return 0.0; // fallback
}

// https://webaudio.github.io/Audio-EQ-Cookbook/audio-eq-cookbook.html
typedef struct BiquadFilter
{
  // biquad equation:
  // y[n] = (b0 * x[n]) + (b1 * x[n-1]) + (b2 * x[n-2]) - (a1 * y[n-1]) - (a2 * y[n-2])
  float b0, b1, b2;
  float a0, a1, a2;
  float x1, x2;
  float y1, y2;
} BiquadFilter;

void BiquadFilter_init(BiquadFilter *self)
{
  self->x1 = 0; // x[n-1]
  self->x2 = 0; // x[n-2]
  self->y1 = 0; // y[n-1]
  self->y2 = 0; // y[n-2]
  self->a0 = 1;
  self->a1 = 0;
  self->a2 = 0;
  self->b0 = 1;
  self->b1 = 0;
  self->b2 = 0;
}
float BiquadFilter_update(BiquadFilter *f, float input, int type, float freq, float Q, float gain)
{
  float omega = 2.0 * fPI * freq / engine.sr;
  float sin_omega = par_sinf(omega);

  // Q conversion: only lowpass/highpass use dB Q (Web Audio API spec)
  // bandpass/notch/allpass/peaking use linear Q (no conversion needed)
  // lowshelf/highshelf don't use Q at all
  if (type == 0 || type == 1)
  {
    Q = pow10(Q / 20.0f);
  }
  float alpha = sin_omega / (2.0f * Q);

  float cos_omega = par_cosf(omega);
  float A, sqrt2Aa, am1w0, ap1w0;

  switch (type)
  {
  case 0: // lowpass
    f->b1 = 1 - cos_omega;
    f->b0 = f->b1 / 2.0;
    f->b2 = f->b0;
    f->a0 = 1 + alpha;
    f->a1 = -2 * cos_omega;
    f->a2 = 1 - alpha;
    break;

  case 1: // highpass
    f->b0 = (1 + cos_omega) / 2.0;
    f->b1 = -(1 + cos_omega);
    f->b2 = f->b0;
    f->a0 = 1 + alpha;
    f->a1 = -2 * cos_omega;
    f->a2 = 1 - alpha;
    break;

  case 2: // band pass (constant skirt gain)
    f->b0 = sin_omega / 2.0;
    f->b1 = 0;
    f->b2 = -f->b0;
    f->a0 = 1 + alpha;
    f->a1 = -2 * cos_omega;
    f->a2 = 1 - alpha;
    break;

  case 3: // notch
    f->b0 = 1;
    f->b1 = -2 * cos_omega;
    f->b2 = 1;
    f->a0 = 1 + alpha;
    f->a1 = -2 * cos_omega;
    f->a2 = 1 - alpha;
    break;

  case 4: // allpass
    f->b0 = 1 - alpha;
    f->b1 = -2 * cos_omega;
    f->b2 = 1 + alpha;
    f->a0 = 1 + alpha;
    f->a1 = -2 * cos_omega;
    f->a2 = 1 - alpha;
    break;

  case 5: // peaking
    A = pow10(gain / 40.0);
    f->b0 = 1 + alpha * A;
    f->b1 = -2 * cos_omega;
    f->b2 = 1 - alpha * A;
    f->a0 = 1 + alpha / A;
    f->a1 = -2 * cos_omega;
    f->a2 = 1 - alpha / A;
    break;

  case 6: // lowshelf
    A = pow10(gain / 40.0);
    sqrt2Aa = 2 * sqrtf(A) * alpha;
    am1w0 = (A - 1) * cos_omega;
    ap1w0 = (A + 1) * cos_omega;
    f->b0 = A * ((A + 1) - am1w0 + sqrt2Aa);
    f->b1 = 2 * A * ((A - 1) - ap1w0);
    f->b2 = A * ((A + 1) - am1w0 - sqrt2Aa);
    f->a0 = (A + 1) + am1w0 + sqrt2Aa;
    f->a1 = -2 * ((A - 1) + ap1w0);
    f->a2 = (A + 1) + am1w0 - sqrt2Aa;
    break;

  case 7: // highshelf
    A = pow10(gain / 40.0);
    sqrt2Aa = 2 * sqrtf(A) * alpha;
    am1w0 = (A - 1) * cos_omega;
    ap1w0 = (A + 1) * cos_omega;
    f->b0 = A * ((A + 1) + am1w0 + sqrt2Aa);
    f->b1 = -2 * A * ((A - 1) + ap1w0);
    f->b2 = A * ((A + 1) + am1w0 - sqrt2Aa);
    f->a0 = (A + 1) - am1w0 + sqrt2Aa;
    f->a1 = 2 * ((A - 1) - ap1w0);
    f->a2 = (A + 1) - am1w0 - sqrt2Aa;
    break;

  default:
    return input;
  }

  f->b0 /= f->a0;
  f->b1 /= f->a0;
  f->b2 /= f->a0;
  f->a1 /= f->a0;
  f->a2 /= f->a0;
  f->a0 = 1.0;

  float output = f->b0 * input +
                 f->b1 * f->x1 +
                 f->b2 * f->x2 -
                 f->a1 * f->y1 -
                 f->a2 * f->y2;

  f->x2 = f->x1;
  f->x1 = input;
  f->y2 = f->y1;
  f->y1 = output;

  return output;
}

typedef struct Lag
{
  float s;
} Lag;

void Lag_init(Lag *self) { self->s = 0; }

float Lag_update(Lag *self, float input, float rate)
{
  // Remap so the useful range is around [0, 1]
  rate = rate * engine.lag_unit;
  if (rate < 1)
    rate = 1;
  self->s += (1 / rate) * (input - self->s);
  return self->s;
}

// sample rate bit crusher
typedef struct Coarse
{
  float hold;
  int t;
} Coarse;

void Coarse_init(Coarse *self)
{
  self->hold = 0.0;
  self->t = 0;
}
float Coarse_update(Coarse *self, float input, int coarse)
{
  if (self->t % coarse == 0)
  {
    self->t = 0;
    self->hold = input;
  }
  self->t++;
  return self->hold;
}

float crush(float input, float crush)
{
  crush = fmaxf(1, crush);
  float x = our_exp2f(crush - 1);
  return roundf(input * x) / x;
}

typedef struct Phaser
{
  BiquadFilter filter1;
  BiquadFilter filter2;
  Phasor lfo;
} Phaser;

void Phaser_init(Phaser *self)
{
  BiquadFilter_init(&self->filter1);
  BiquadFilter_init(&self->filter2);
  Phasor_init(&self->lfo);
}

float Phaser_update(Phaser *self, float input, float rate, float depth,
                    float center, float sweep)
{
  float lfo_val = SineOsc_update(&self->lfo, rate);

  // Calculate Q from depth (matches superdough)
  float Q = 2.0f - fminf(fmaxf(depth * 2.0f, 0.0f), 1.9f);

  // Apply LFO to frequency using exponential detune (matches superdough)
  float detune_cents = lfo_val * sweep;
  float detune_multiplier = our_exp2f(detune_cents / 1200.0f);

  float freq1 = center * detune_multiplier;
  float freq2 = (center + 282.0f) * detune_multiplier;

  freq1 = fmaxf(freq1, 20.0f);
  freq1 = fminf(freq1, engine.sr * 0.45f);
  freq2 = fmaxf(freq2, 20.0f);
  freq2 = fminf(freq2, engine.sr * 0.45f);

  float out = BiquadFilter_update(&self->filter1, input, 3, freq1, Q, 0.0f);
  out = BiquadFilter_update(&self->filter2, out, 3, freq2, Q, 0.0f);

  return out;
}

#define FLANGER_MAX_DELAY_MS 10.0f
#define FLANGER_BUFFER_SIZE 512

typedef struct Flanger
{
  float buffer[FLANGER_BUFFER_SIZE];
  int write_pos;
  Phasor lfo;
  float feedback_buffer;
} Flanger;

void Flanger_init(Flanger *self)
{
  for (int i = 0; i < FLANGER_BUFFER_SIZE; i++)
    self->buffer[i] = 0.0f;
  self->write_pos = 0;
  Phasor_init(&self->lfo);
  self->feedback_buffer = 0.0f;
}

float Flanger_update(Flanger *self, float input, float rate, float depth, float feedback)
{
  float lfo_val = SineOsc_update(&self->lfo, rate);

  float min_delay_ms = 0.5f;
  float max_delay_ms = FLANGER_MAX_DELAY_MS;
  float delay_range = max_delay_ms - min_delay_ms;

  float depth_curve = depth * depth;
  float delay_ms = min_delay_ms + depth_curve * delay_range * (lfo_val * 0.5f + 0.5f);

  float delay_samples = delay_ms * engine.sr * 0.001f;
  delay_samples = fmaxf(1.0f, fminf(delay_samples, FLANGER_BUFFER_SIZE - 2.0f));

  int read_pos_int = (int)floorf(delay_samples);
  float frac = delay_samples - read_pos_int;

  int read_index1 = (self->write_pos - read_pos_int + FLANGER_BUFFER_SIZE) % FLANGER_BUFFER_SIZE;
  int read_index2 = (self->write_pos - read_pos_int - 1 + FLANGER_BUFFER_SIZE) % FLANGER_BUFFER_SIZE;

  float delayed1 = self->buffer[read_index1];
  float delayed2 = self->buffer[read_index2];
  float delayed = delayed1 + frac * (delayed2 - delayed1);

  feedback = fmaxf(0.0f, fminf(feedback, 0.95f));

  self->buffer[self->write_pos] = input + self->feedback_buffer * feedback;
  self->write_pos = (self->write_pos + 1) % FLANGER_BUFFER_SIZE;

  self->feedback_buffer = delayed;

  return input * 0.5f + delayed * 0.5f;
}

#define CHORUS_MAX_DELAY_MS 50.0f
#define CHORUS_BUFFER_SIZE 2048
#define CHORUS_VOICES 3

typedef struct Chorus
{
  float buffer[CHORUS_BUFFER_SIZE];
  int write_pos;
  Phasor lfo[CHORUS_VOICES];
} Chorus;

void Chorus_init(Chorus *self)
{
  for (int i = 0; i < CHORUS_BUFFER_SIZE; i++)
    self->buffer[i] = 0.0f;
  self->write_pos = 0;
  for (int i = 0; i < CHORUS_VOICES; i++)
  {
    Phasor_init(&self->lfo[i]);
    self->lfo[i].phase = (float)i / (float)CHORUS_VOICES;
  }
}

void Chorus_update(Chorus *self, float *left, float *right, float rate, float depth, float delay_ms)
{
  float min_ms = 1.5f;
  float max_ms = fminf(delay_ms * 2.0f, CHORUS_MAX_DELAY_MS);
  float range = max_ms - min_ms;

  depth = fmaxf(0.0f, fminf(depth, 1.0f));

  float mono = (*left + *right) * 0.5f;
  self->buffer[self->write_pos] = mono;

  float outL = 0.0f;
  float outR = 0.0f;

  for (int v = 0; v < CHORUS_VOICES; v++)
  {
    float lfo = SineOsc_update(&self->lfo[v], rate);

    float dlyL = delay_ms + depth * range * lfo * 0.5f;
    float dlyR = delay_ms - depth * range * lfo * 0.5f;

    dlyL = fmaxf(min_ms, fminf(dlyL, max_ms));
    dlyR = fmaxf(min_ms, fminf(dlyR, max_ms));

    float sampL = fmaxf(1.0f, fminf(dlyL * engine.sr * 0.001f, CHORUS_BUFFER_SIZE - 2.0f));
    float sampR = fmaxf(1.0f, fminf(dlyR * engine.sr * 0.001f, CHORUS_BUFFER_SIZE - 2.0f));

    int posL = (int)floorf(sampL);
    float fracL = sampL - posL;
    int idxL0 = (self->write_pos - posL + CHORUS_BUFFER_SIZE) % CHORUS_BUFFER_SIZE;
    int idxL1 = (self->write_pos - posL - 1 + CHORUS_BUFFER_SIZE) % CHORUS_BUFFER_SIZE;
    float tapL = self->buffer[idxL0] + fracL * (self->buffer[idxL1] - self->buffer[idxL0]);

    int posR = (int)floorf(sampR);
    float fracR = sampR - posR;
    int idxR0 = (self->write_pos - posR + CHORUS_BUFFER_SIZE) % CHORUS_BUFFER_SIZE;
    int idxR1 = (self->write_pos - posR - 1 + CHORUS_BUFFER_SIZE) % CHORUS_BUFFER_SIZE;
    float tapR = self->buffer[idxR0] + fracR * (self->buffer[idxR1] - self->buffer[idxR0]);

    outL += tapL;
    outR += tapR;
  }

  self->write_pos = (self->write_pos + 1) % CHORUS_BUFFER_SIZE;

  outL /= (float)CHORUS_VOICES;
  outR /= (float)CHORUS_VOICES;

  float dry = sqrtf(0.5f);
  float wet = sqrtf(0.5f);

  *left = mono * dry + outL * wet;
  *right = mono * dry + outR * wet;
}

float distort(float input, float amount, float postgain)
{
  float k = our_expm1f(amount); // from superdough
  // float k = (2 * amount) / (1 - amount); // from noisecraft
  return (((1 + k) * input) / (1 + k * fabsf(input))) * postgain;
}

typedef struct Delay
{
  int write;
  int read;
  float *buffer;
} Delay;

void Delay_init(Delay *this)
{
  this->write = 0;
  this->read = 0;
}

float Delay_update(Delay *this, float input, float time, int max_delay_samples)
{

  this->write = (this->write + 1) % max_delay_samples;
  this->buffer[this->write] = input;

  int numSamples = MIN(floor(engine.sr * time), max_delay_samples - 1);

  this->read = this->write - numSamples;

  if (this->read < 0)
    this->read += max_delay_samples;

  return this->buffer[this->read];
}

// ============================================================================
// REVERB (Dattorro)
// ============================================================================
// Jon Dattorro reverb implementation
// Algorithm: https://ccrma.stanford.edu/~dattorro/EffectDesignPart1.pdf
// Adapted from el-visio's implementation (github.com/el-visio), MIT License
// Mono input -> stereo output reverb using feedback delay network
// Three components: delay lines, all-pass filters, low-pass filters

#define VERB_MAX_PREDELAY 4800

typedef enum
{
  VERB_TAP_MAIN = 0,
  VERB_TAP_OUT1,
  VERB_TAP_OUT2,
  VERB_TAP_OUT3,
  VERB_MAX_TAPS
} VerbTap;

typedef struct
{
  float *buffer; // Sample buffer (pre-allocated)
  uint16_t mask;
  uint16_t readOffset[VERB_MAX_TAPS]; // Read positions for multiple taps
} VerbDelayBuffer;

typedef struct
{
  // Feedback network components
  VerbDelayBuffer preDelay;
  VerbDelayBuffer inDiffusion[4];
  VerbDelayBuffer decayDiffusion1[2];
  VerbDelayBuffer preDampingDelay[2];
  VerbDelayBuffer decayDiffusion2[2];
  VerbDelayBuffer postDampingDelay[2];

  // Filter states
  float preFilter;
  float damping[2];

  // Parameters
  float preFilterAmount;
  float inputDiffusion1Amount;
  float inputDiffusion2Amount;
  float decayDiffusion1Amount;
  float dampingAmount;
  float decayAmount;
  float decayDiffusion2Amount;

  // Time counter for delay synchronization
  uint16_t t;
} DattorroVerb;

static inline void VerbDelayBuffer_setDelay(VerbDelayBuffer *db, int tap, uint16_t delay)
{
  db->readOffset[tap] = db->mask + 1 - delay;
}

static inline void VerbDelayBuffer_init(VerbDelayBuffer *db, float *buffer, uint16_t delay)
{
  uint16_t numBits = 0;
  uint16_t x = delay;

  // Calculate number of bits for buffer size (always power of 2)
  while (x)
  {
    numBits++;
    x >>= 1;
  }

  uint16_t bufferSize = 1 << numBits;
  db->buffer = buffer;
  db->mask = bufferSize - 1;

  // Clear buffer
  for (int i = 0; i < bufferSize; i++)
  {
    db->buffer[i] = 0.0f;
  }

  VerbDelayBuffer_setDelay(db, VERB_TAP_MAIN, delay);
}

static inline void VerbDelayBuffer_write(VerbDelayBuffer *db, uint16_t t, float in)
{
  db->buffer[t & db->mask] = in;
}

static inline float VerbDelayBuffer_read(VerbDelayBuffer *db, int tapId, uint16_t t)
{
  return db->buffer[(t + db->readOffset[tapId]) & db->mask];
}

static inline float VerbDelayBuffer_process(VerbDelayBuffer *db, uint16_t t, float in)
{
  db->buffer[t & db->mask] = in;
  return db->buffer[(t + db->readOffset[VERB_TAP_MAIN]) & db->mask];
}

static inline float VerbAllPassFilter_process(VerbDelayBuffer *db, uint16_t t, float gain, float in)
{
  float delayed = VerbDelayBuffer_read(db, VERB_TAP_MAIN, t);
  in += delayed * -gain;
  VerbDelayBuffer_write(db, t, in);
  return delayed + in * gain;
}

static inline float VerbLowPassFilter_process(float *out, float freq, float in)
{
  *out += (in - *out) * freq;
  return *out;
}

void DattorroVerb_setPreDelay(DattorroVerb *v, float value)
{
  VerbDelayBuffer_setDelay(&v->preDelay, VERB_TAP_MAIN, value * VERB_MAX_PREDELAY);
}

void DattorroVerb_setPreFilter(DattorroVerb *v, float value)
{
  v->preFilterAmount = value;
}

void DattorroVerb_setInputDiffusion1(DattorroVerb *v, float value)
{
  v->inputDiffusion1Amount = value;
}

void DattorroVerb_setInputDiffusion2(DattorroVerb *v, float value)
{
  v->inputDiffusion2Amount = value;
}

void DattorroVerb_setDecayDiffusion(DattorroVerb *v, float value)
{
  v->decayDiffusion1Amount = value;
}

void DattorroVerb_setDecay(DattorroVerb *v, float value)
{
  v->decayAmount = clamp(value, 0.0f, 0.99f);
  v->decayDiffusion2Amount = clamp(v->decayAmount + 0.15f, 0.25f, 0.50f);
}

void DattorroVerb_setDamping(DattorroVerb *v, float value)
{
  v->dampingAmount = value;
}

void DattorroVerb_init(DattorroVerb *v, float *mem_pool)
{
  // Clear struct
  v->preFilter = 0.0f;
  v->damping[0] = 0.0f;
  v->damping[1] = 0.0f;
  v->t = 0;

  // Calculate buffer sizes (powers of 2)
  int bufferSizes[] = {
      1 << 13, // preDelay: 8192 (for 4800)
      1 << 8,  // inDiffusion[0]: 256 (for 142)
      1 << 7,  // inDiffusion[1]: 128 (for 107)
      1 << 9,  // inDiffusion[2]: 512 (for 379)
      1 << 9,  // inDiffusion[3]: 512 (for 277)
      1 << 10, // decayDiffusion1[0]: 1024 (for 672)
      1 << 13, // preDampingDelay[0]: 8192 (for 4453)
      1 << 11, // decayDiffusion2[0]: 2048 (for 1800)
      1 << 12, // postDampingDelay[0]: 4096 (for 3720)
      1 << 10, // decayDiffusion1[1]: 1024 (for 908)
      1 << 13, // preDampingDelay[1]: 8192 (for 4217)
      1 << 12, // decayDiffusion2[1]: 4096 (for 2656)
      1 << 12  // postDampingDelay[1]: 4096 (for 3163)
  };

  // Assign buffers from memory pool
  float *ptr = mem_pool;
  VerbDelayBuffer_init(&v->preDelay, ptr, VERB_MAX_PREDELAY);
  ptr += bufferSizes[0];

  VerbDelayBuffer_init(&v->inDiffusion[0], ptr, 142);
  ptr += bufferSizes[1];
  VerbDelayBuffer_init(&v->inDiffusion[1], ptr, 107);
  ptr += bufferSizes[2];
  VerbDelayBuffer_init(&v->inDiffusion[2], ptr, 379);
  ptr += bufferSizes[3];
  VerbDelayBuffer_init(&v->inDiffusion[3], ptr, 277);
  ptr += bufferSizes[4];

  VerbDelayBuffer_init(&v->decayDiffusion1[0], ptr, 672);
  ptr += bufferSizes[5];

  VerbDelayBuffer_init(&v->preDampingDelay[0], ptr, 4453);
  ptr += bufferSizes[6];
  VerbDelayBuffer_setDelay(&v->preDampingDelay[0], VERB_TAP_OUT1, 353);
  VerbDelayBuffer_setDelay(&v->preDampingDelay[0], VERB_TAP_OUT2, 3627);
  VerbDelayBuffer_setDelay(&v->preDampingDelay[0], VERB_TAP_OUT3, 1990);

  VerbDelayBuffer_init(&v->decayDiffusion2[0], ptr, 1800);
  ptr += bufferSizes[7];
  VerbDelayBuffer_setDelay(&v->decayDiffusion2[0], VERB_TAP_OUT1, 187);
  VerbDelayBuffer_setDelay(&v->decayDiffusion2[0], VERB_TAP_OUT2, 1228);

  VerbDelayBuffer_init(&v->postDampingDelay[0], ptr, 3720);
  ptr += bufferSizes[8];
  VerbDelayBuffer_setDelay(&v->postDampingDelay[0], VERB_TAP_OUT1, 1066);
  VerbDelayBuffer_setDelay(&v->postDampingDelay[0], VERB_TAP_OUT2, 2673);

  VerbDelayBuffer_init(&v->decayDiffusion1[1], ptr, 908);
  ptr += bufferSizes[9];

  VerbDelayBuffer_init(&v->preDampingDelay[1], ptr, 4217);
  ptr += bufferSizes[10];
  VerbDelayBuffer_setDelay(&v->preDampingDelay[1], VERB_TAP_OUT1, 266);
  VerbDelayBuffer_setDelay(&v->preDampingDelay[1], VERB_TAP_OUT2, 2974);
  VerbDelayBuffer_setDelay(&v->preDampingDelay[1], VERB_TAP_OUT3, 2111);

  VerbDelayBuffer_init(&v->decayDiffusion2[1], ptr, 2656);
  ptr += bufferSizes[11];
  VerbDelayBuffer_setDelay(&v->decayDiffusion2[1], VERB_TAP_OUT1, 335);
  VerbDelayBuffer_setDelay(&v->decayDiffusion2[1], VERB_TAP_OUT2, 1913);

  VerbDelayBuffer_init(&v->postDampingDelay[1], ptr, 3163);
  ptr += bufferSizes[12];
  VerbDelayBuffer_setDelay(&v->postDampingDelay[1], VERB_TAP_OUT1, 121);
  VerbDelayBuffer_setDelay(&v->postDampingDelay[1], VERB_TAP_OUT2, 1996);

  // Default parameters
  DattorroVerb_setPreDelay(v, 0.1f);
  DattorroVerb_setPreFilter(v, 0.85f);
  DattorroVerb_setInputDiffusion1(v, 0.75f);
  DattorroVerb_setInputDiffusion2(v, 0.625f);
  DattorroVerb_setDecay(v, 0.75f);
  DattorroVerb_setDecayDiffusion(v, 0.70f);
  DattorroVerb_setDamping(v, 0.95f);
}

int DattorroVerb_getMemorySize(void)
{
  // Returns total memory needed for all delay buffers (in floats)
  int bufferSizes[] = {
      1 << 13, // preDelay
      1 << 8,  // inDiffusion[0]
      1 << 7,  // inDiffusion[1]
      1 << 9,  // inDiffusion[2]
      1 << 9,  // inDiffusion[3]
      1 << 10, // decayDiffusion1[0]
      1 << 13, // preDampingDelay[0]
      1 << 11, // decayDiffusion2[0]
      1 << 12, // postDampingDelay[0]
      1 << 10, // decayDiffusion1[1]
      1 << 13, // preDampingDelay[1]
      1 << 12, // decayDiffusion2[1]
      1 << 12  // postDampingDelay[1]
  };

  int total = 0;
  for (int i = 0; i < 13; i++)
  {
    total += bufferSizes[i];
  }
  return total;
}

void DattorroVerb_process(DattorroVerb *v, float in)
{
  float x, x1;

  // Modulate decayDiffusion1 delay lines
  if ((v->t & 0x07ff) == 0)
  {
    if (v->t < (1 << 15))
    {
      v->decayDiffusion1[0].readOffset[VERB_TAP_MAIN]--;
      v->decayDiffusion1[1].readOffset[VERB_TAP_MAIN]--;
    }
    else
    {
      v->decayDiffusion1[0].readOffset[VERB_TAP_MAIN]++;
      v->decayDiffusion1[1].readOffset[VERB_TAP_MAIN]++;
    }
  }

  // Pre-delay
  x = VerbDelayBuffer_process(&v->preDelay, v->t, in);

  // Pre-filter (low-pass)
  x = VerbLowPassFilter_process(&v->preFilter, v->preFilterAmount, x);

  // Input diffusion (4 all-pass filters)
  x = VerbAllPassFilter_process(&v->inDiffusion[0], v->t, v->inputDiffusion1Amount, x);
  x = VerbAllPassFilter_process(&v->inDiffusion[1], v->t, v->inputDiffusion1Amount, x);
  x = VerbAllPassFilter_process(&v->inDiffusion[2], v->t, v->inputDiffusion2Amount, x);
  x = VerbAllPassFilter_process(&v->inDiffusion[3], v->t, v->inputDiffusion2Amount, x);

  // Process both halves of reverberation tank
  for (int i = 0; i < 2; i++)
  {
    // Add cross feedback from opposite side
    x1 = x + VerbDelayBuffer_read(&v->postDampingDelay[1 - i], VERB_TAP_MAIN, v->t) * v->decayAmount;

    // Process single tank half
    x1 = VerbAllPassFilter_process(&v->decayDiffusion1[i], v->t, -v->decayDiffusion1Amount, x1);
    x1 = VerbDelayBuffer_process(&v->preDampingDelay[i], v->t, x1);
    x1 = VerbLowPassFilter_process(&v->damping[i], v->dampingAmount, x1);
    x1 *= v->decayAmount;
    x1 = VerbAllPassFilter_process(&v->decayDiffusion2[i], v->t, v->decayDiffusion2Amount, x1);
    VerbDelayBuffer_write(&v->postDampingDelay[i], v->t, x1);
  }

  // Increment time counter
  v->t++;
}

float DattorroVerb_getLeft(DattorroVerb *v)
{
  float a = 0.0f;
  a += VerbDelayBuffer_read(&v->preDampingDelay[1], VERB_TAP_OUT1, v->t);
  a += VerbDelayBuffer_read(&v->preDampingDelay[1], VERB_TAP_OUT2, v->t);
  a -= VerbDelayBuffer_read(&v->decayDiffusion2[1], VERB_TAP_OUT2, v->t);
  a += VerbDelayBuffer_read(&v->postDampingDelay[1], VERB_TAP_OUT2, v->t);
  a -= VerbDelayBuffer_read(&v->preDampingDelay[0], VERB_TAP_OUT3, v->t);
  a -= VerbDelayBuffer_read(&v->decayDiffusion2[0], VERB_TAP_OUT1, v->t);
  a += VerbDelayBuffer_read(&v->postDampingDelay[0], VERB_TAP_OUT1, v->t);
  return a;
}

float DattorroVerb_getRight(DattorroVerb *v)
{
  float a = 0.0f;
  a += VerbDelayBuffer_read(&v->preDampingDelay[0], VERB_TAP_OUT1, v->t);
  a += VerbDelayBuffer_read(&v->preDampingDelay[0], VERB_TAP_OUT2, v->t);
  a -= VerbDelayBuffer_read(&v->decayDiffusion2[0], VERB_TAP_OUT2, v->t);
  a += VerbDelayBuffer_read(&v->postDampingDelay[0], VERB_TAP_OUT2, v->t);
  a -= VerbDelayBuffer_read(&v->preDampingDelay[1], VERB_TAP_OUT3, v->t);
  a -= VerbDelayBuffer_read(&v->decayDiffusion2[1], VERB_TAP_OUT1, v->t);
  a += VerbDelayBuffer_read(&v->postDampingDelay[1], VERB_TAP_OUT1, v->t);
  return a;
}

// ---
// event system
// ---

typedef enum
{
  UNKNOWN_SOURCE = -1,
  TRI_OSC,
  SINE_OSC,
  SAW_OSC,
  ZAW_OSC,
  PULSE_OSC,
  PULZE_OSC,
  WHITE_NOISE,
  PINK_NOISE,
  BROWN_NOISE,
  CONST,
  FILE_SRC,
} Source;

Source get_source(char *name)
{

  if (strcmp(name, "triangle") == 0 || strcmp(name, "tri") == 0)
    return TRI_OSC;
  if (strcmp(name, "sine") == 0)
    return SINE_OSC;
  if (strcmp(name, "sawtooth") == 0 || strcmp(name, "saw") == 0)
    return SAW_OSC;
  if (strcmp(name, "zawtooth") == 0 || strcmp(name, "zaw") == 0)
    return ZAW_OSC;
  if (strcmp(name, "pulse") == 0 || strcmp(name, "square") == 0)
    return PULSE_OSC;
  if (strcmp(name, "pulze") == 0 || strcmp(name, "zquare") == 0)
    return PULZE_OSC;
  if (strcmp(name, "white") == 0)
    return WHITE_NOISE;
  if (strcmp(name, "pink") == 0)
    return PINK_NOISE;
  if (strcmp(name, "brown") == 0)
    return BROWN_NOISE;
  if (strcmp(name, "const") == 0)
    return CONST;
  return UNKNOWN_SOURCE;
}

typedef enum
{
  CMD_UNKNOWN = -1,
  CMD_PLAY,
  CMD_UPDATE_ALL,
  CMD_UPDATE_GROUP,
  CMD_PANIC,
  CMD_HUSH,
  CMD_HUSH_ENDLESS,
  CMD_HUSH_GROUP,
  CMD_RESET,
  CMD_RESET_TIME,
  CMD_RESET_SCHEDULE,
} Command;

Command get_command(char *name)
{
  if (strcmp(name, "play") == 0)
    return CMD_PLAY;
  if (strcmp(name, "update_all") == 0)
    return CMD_UPDATE_ALL;
  if (strcmp(name, "update_group") == 0)
    return CMD_UPDATE_GROUP;
  if (strcmp(name, "panic") == 0)
    return CMD_PANIC;
  if (strcmp(name, "hush") == 0)
    return CMD_HUSH;
  if (strcmp(name, "hush_endless") == 0)
    return CMD_HUSH_ENDLESS;
  if (strcmp(name, "hush_group") == 0)
    return CMD_HUSH_GROUP;
  if (strcmp(name, "reset") == 0)
    return CMD_RESET;
  if (strcmp(name, "reset_time") == 0)
    return CMD_RESET_TIME;
  if (strcmp(name, "reset_schedule") == 0)
    return CMD_RESET_SCHEDULE;
  return CMD_UNKNOWN;
}

int get_ftype(char *name)
{
  if (strcmp(name, "12db") == 0)
    return 0;
  if (strcmp(name, "24db") == 0)
    return 1;
  if (strcmp(name, "48db") == 0)
    return 2;
  return atoi(name); // fallback to numeric parsing
}

// this is used for setting an event via a string
#define EVENT_INPUT_SIZE 1024
char event_input[EVENT_INPUT_SIZE];

typedef struct
{
  Command cmd;
  int group;
  int voices; // max polyphony in group
  int reset;
  float freq;
  float rate; // derived freq
  float speed;

  float glide;
  bool glide_active;

  float gain;
  float pan;
  float velocity;
  float postgain;
  bool gain_adsr_active;
  float gate;
  float duration; // gate duration
  Source sound;
  FileSource file_source;
  int file; // file_index
  float pw;

  float attack;
  float decay;
  float sustain;
  float release;

  float lpf;
  float lpq;
  bool lp_active;
  float lpe;
  float lpa;
  float lpd;
  float lps;
  float lpr;
  bool lp_adsr_active;

  float hpf;
  float hpq;
  bool hp_active;
  float hpe;
  float hpa;
  float hpd;
  float hps;
  float hpr;
  bool hp_adsr_active;

  float bpf;
  float bpq;
  bool bp_active;
  float bpe;
  float bpa;
  float bpd;
  float bps;
  float bpr;
  bool bp_adsr_active;

  int ftype;

  float penv;
  float patt;
  float pdec;
  float psus;
  float prel;
  bool p_adsr_active;

  float vib;
  float vibmod;
  bool vib_active;

  float fm;
  float fmh;
  bool fm_active;
  float fme;
  float fma;
  float fmd;
  float fms;
  float fmr;
  bool fm_adsr_active;

  float am;
  float amdepth;
  bool am_active;

  float rm;
  float rmdepth;
  bool rm_active;

  float phaser;
  float phaserdepth;
  float phasersweep;
  float phasercenter;
  bool phaser_active;

  float flanger;
  float flangerdepth;
  float flangerfeedback;
  bool flanger_active;

  float chorus;
  float chorusdepth;
  float chorusdelay;
  bool chorus_active;

  float begin;
  float end;

  float coarse;
  bool coarse_active;

  float crush;
  bool crush_active;

  float distort;
  float distortvol;
  bool distort_active;

  float delay;
  float delaytime;
  float delayfeedback;
  bool delay_active;

  float verb;
  float verbdecay;
  float verbdamp;
  float verbpredelay;
  float verbdiff;
  bool verb_active;

  int orbit;
  int channels;
  double time;
  float repeat;
  float out;
} Event;

void reset_event(Event *p)
{
  p->file = -1;
  p->time = NAN;
  p->repeat = NAN;
  p->orbit = -1;

  p->group = -1;
  p->voices = -1;
  p->freq = NAN;
  p->speed = NAN;
  p->glide = NAN;
  p->glide_active = false;

  p->gain = NAN;
  p->pan = NAN;
  p->gate = NAN;
  p->velocity = NAN;
  p->postgain = NAN;
  p->duration = NAN;
  p->sound = UNKNOWN_SOURCE;
  p->file_source.pcm = NULL;
  p->pw = NAN;
  p->reset = -1;

  p->attack = NAN;
  p->decay = NAN;
  p->sustain = NAN;
  p->release = NAN;
  p->gain_adsr_active = false;

  p->lpf = NAN;
  p->lpq = NAN;
  p->lpe = NAN;
  p->lpa = NAN;
  p->lpd = NAN;
  p->lps = NAN;
  p->lpr = NAN;
  p->lp_active = false;
  p->lp_adsr_active = false;

  p->hpf = NAN;
  p->hpq = NAN;
  p->hpe = NAN;
  p->hpa = NAN;
  p->hpd = NAN;
  p->hps = NAN;
  p->hpr = NAN;
  p->hp_active = false;
  p->hp_adsr_active = false;

  p->bpf = NAN;
  p->bpq = NAN;
  p->bpe = NAN;
  p->bpa = NAN;
  p->bpd = NAN;
  p->bps = NAN;
  p->bpr = NAN;
  p->bp_active = false;
  p->bp_adsr_active = false;

  p->ftype = -1;

  p->penv = NAN;
  p->patt = NAN;
  p->pdec = NAN;
  p->psus = NAN;
  p->prel = NAN;
  p->p_adsr_active = false;

  p->vib = NAN;
  p->vibmod = NAN;
  p->vib_active = false;

  p->fm = NAN;
  p->fmh = NAN;
  p->fm_active = false;
  p->fme = NAN;
  p->fma = NAN;
  p->fmd = NAN;
  p->fms = NAN;
  p->fmr = NAN;
  p->fm_adsr_active = false;

  p->am = NAN;
  p->amdepth = NAN;
  p->am_active = false;

  p->rm = NAN;
  p->rmdepth = NAN;
  p->rm_active = false;

  p->phaser = NAN;
  p->phaserdepth = NAN;
  p->phasersweep = NAN;
  p->phasercenter = NAN;
  p->phaser_active = false;

  p->flanger = NAN;
  p->flangerdepth = NAN;
  p->flangerfeedback = NAN;
  p->flanger_active = false;

  p->chorus = NAN;
  p->chorusdepth = NAN;
  p->chorusdelay = NAN;
  p->chorus_active = false;

  p->begin = NAN;
  p->end = NAN;

  p->crush = NAN;
  p->crush_active = false;

  p->coarse = NAN;
  p->coarse_active = false;

  p->distort = NAN;
  p->distortvol = NAN;
  p->distort_active = false;

  p->delay = NAN;
  p->delaytime = NAN;
  p->delayfeedback = NAN;
  p->delay_active = false;

  p->verb = NAN;
  p->verbdecay = NAN;
  p->verbdamp = NAN;
  p->verbpredelay = NAN;
  p->verbdiff = NAN;
  p->verb_active = false;

  p->cmd = CMD_PLAY;
}

// assigns defined params of source to target
void merge_event(const Event *source, Event *target)
{
  // compared to reset_event, some fields are not here on purpose
  // fields like time and cmd dont make sense to merge while a voice runs..
  if (source->orbit != -1)
    target->orbit = source->orbit;
  if (source->sound != -1)
    target->sound = source->sound;
  if (source->ftype != -1)
    target->ftype = source->ftype;

  if (!isnan(source->freq))
    target->freq = source->freq;
  if (!isnan(source->speed))
    target->speed = source->speed;
  if (!isnan(source->glide))
    target->glide = source->glide;
  if (!isnan(source->gain))
    target->gain = source->gain;
  if (!isnan(source->pan))
    target->pan = source->pan;
  if (!isnan(source->gate))
    target->gate = source->gate;
  if (!isnan(source->velocity))
    target->velocity = source->velocity;
  if (!isnan(source->postgain))
    target->postgain = source->postgain;
  if (!isnan(source->duration))
    target->duration = source->duration;
  if (!isnan(source->pw))
    target->pw = source->pw;
  if (!isnan(source->attack))
    target->attack = source->attack;
  if (!isnan(source->decay))
    target->decay = source->decay;
  if (!isnan(source->sustain))
    target->sustain = source->sustain;
  if (!isnan(source->release))
    target->release = source->release;
  if (!isnan(source->lpf))
    target->lpf = source->lpf;
  if (!isnan(source->lpq))
    target->lpq = source->lpq;
  if (!isnan(source->lpe))
    target->lpe = source->lpe;
  if (!isnan(source->lpa))
    target->lpa = source->lpa;
  if (!isnan(source->lpd))
    target->lpd = source->lpd;
  if (!isnan(source->lps))
    target->lps = source->lps;
  if (!isnan(source->lpr))
    target->lpr = source->lpr;
  if (!isnan(source->hpf))
    target->hpf = source->hpf;
  if (!isnan(source->hpq))
    target->hpq = source->hpq;
  if (!isnan(source->hpe))
    target->hpe = source->hpe;
  if (!isnan(source->hpa))
    target->hpa = source->hpa;
  if (!isnan(source->hpd))
    target->hpd = source->hpd;
  if (!isnan(source->hps))
    target->hps = source->hps;
  if (!isnan(source->hpr))
    target->hpr = source->hpr;
  if (!isnan(source->bpf))
    target->bpf = source->bpf;
  if (!isnan(source->bpq))
    target->bpq = source->bpq;
  if (!isnan(source->bpe))
    target->bpe = source->bpe;
  if (!isnan(source->bpa))
    target->bpa = source->bpa;
  if (!isnan(source->bpd))
    target->bpd = source->bpd;
  if (!isnan(source->bps))
    target->bps = source->bps;
  if (!isnan(source->bpr))
    target->bpr = source->bpr;
  if (!isnan(source->penv))
    target->penv = source->penv;
  if (!isnan(source->patt))
    target->patt = source->patt;
  if (!isnan(source->pdec))
    target->pdec = source->pdec;
  if (!isnan(source->psus))
    target->psus = source->psus;
  if (!isnan(source->prel))
    target->prel = source->prel;
  if (!isnan(source->vib))
    target->vib = source->vib;
  if (!isnan(source->vibmod))
    target->vibmod = source->vibmod;
  if (!isnan(source->fm))
    target->fm = source->fm;
  if (!isnan(source->fmh))
    target->fmh = source->fmh;
  if (!isnan(source->fme))
    target->fme = source->fme;
  if (!isnan(source->fma))
    target->fma = source->fma;
  if (!isnan(source->fmd))
    target->fmd = source->fmd;
  if (!isnan(source->fms))
    target->fms = source->fms;
  if (!isnan(source->fmr))
    target->fmr = source->fmr;
  if (!isnan(source->am))
    target->am = source->am;
  if (!isnan(source->amdepth))
    target->amdepth = source->amdepth;
  if (!isnan(source->rm))
    target->rm = source->rm;
  if (!isnan(source->rmdepth))
    target->rmdepth = source->rmdepth;
  if (!isnan(source->phaser))
    target->phaser = source->phaser;
  if (!isnan(source->phaserdepth))
    target->phaserdepth = source->phaserdepth;
  if (!isnan(source->phasersweep))
    target->phasersweep = source->phasersweep;
  if (!isnan(source->phasercenter))
    target->phasercenter = source->phasercenter;
  if (!isnan(source->flanger))
    target->flanger = source->flanger;
  if (!isnan(source->flangerdepth))
    target->flangerdepth = source->flangerdepth;
  if (!isnan(source->flangerfeedback))
    target->flangerfeedback = source->flangerfeedback;
  if (!isnan(source->chorus))
    target->chorus = source->chorus;
  if (!isnan(source->chorusdepth))
    target->chorusdepth = source->chorusdepth;
  if (!isnan(source->chorusdelay))
    target->chorusdelay = source->chorusdelay;
  if (!isnan(source->begin))
    target->begin = source->begin;
  if (!isnan(source->end))
    target->end = source->end;
  if (!isnan(source->crush))
    target->crush = source->crush;
  if (!isnan(source->distort))
    target->distort = source->distort;
  if (!isnan(source->distortvol))
    target->distortvol = source->distortvol;
  if (!isnan(source->delay))
    target->delay = source->delay;
  if (!isnan(source->delaytime))
    target->delaytime = source->delaytime;
  if (!isnan(source->delayfeedback))
    target->delayfeedback = source->delayfeedback;
  if (!isnan(source->verb))
    target->verb = source->verb;
  if (!isnan(source->verbdecay))
    target->verbdecay = source->verbdecay;
  if (!isnan(source->verbdamp))
    target->verbdamp = source->verbdamp;
  if (!isnan(source->verbpredelay))
    target->verbpredelay = source->verbpredelay;
  if (!isnan(source->verbdiff))
    target->verbdiff = source->verbdiff;
}

float midi2freq(float midi)
{
  return our_exp2f((midi - 69.0) / 12.0) * 440.0;
}

void parse_event(Event *p, const char *msg)
{
  char buf[EVENT_INPUT_SIZE];
  strncpy(buf, msg, sizeof(buf) - 1);
  buf[sizeof(buf) - 1] = '\0';
  char *token = strtok(buf, "/");
  int index = 0;
  char *key = NULL;
  while (token != NULL)
  {
    // even index = key
    if (index % 2 == 0)
      key = token;
    // odd index = value
    else if (strcmp(key, "dough") == 0 || strcmp(key, "dirt") == 0)
      p->cmd = get_command(token);
    else if (strcmp(key, "time") == 0 || strcmp(key, "t") == 0)
      p->time = atof(token);
    else if (strcmp(key, "repeat") == 0 || strcmp(key, "rep") == 0)
      p->repeat = atof(token);
    else if (strcmp(key, "gate") == 0)
      p->gate = atof(token);
    else if (strcmp(key, "duration") == 0 || strcmp(key, "d") == 0)
      p->duration = atof(token);
    else if (strcmp(key, "group") == 0)
      p->group = atoi(token);
    else if (strcmp(key, "voices") == 0)
      p->voices = atoi(token);
    else if (strcmp(key, "freq") == 0)
      p->freq = atof(token);
    else if (strcmp(key, "note") == 0)
      p->freq = midi2freq(atof(token));
    else if (strcmp(key, "speed") == 0)
      p->speed = atof(token);
    else if (strcmp(key, "glide") == 0)
      p->glide = atof(token);
    else if (strcmp(key, "sound") == 0 || strcmp(key, "s") == 0)
      p->sound = get_source(token);
    else if (strcmp(key, "file") == 0)
      p->file = atoi(token);
    else if (strcmp(key, "pw") == 0)
      p->pw = atof(token);
    else if (strcmp(key, "lpf") == 0 || strcmp(key, "cutoff") == 0)
      p->lpf = atof(token);
    else if (strcmp(key, "lpq") == 0 || strcmp(key, "resonance") == 0)
      p->lpq = atof(token);
    else if (strcmp(key, "lpa") == 0 || strcmp(key, "lpattack") == 0)
      p->lpa = atof(token);
    else if (strcmp(key, "lpd") == 0 || strcmp(key, "lpdecay") == 0)
      p->lpd = atof(token);
    else if (strcmp(key, "lps") == 0 || strcmp(key, "lpsustain") == 0)
      p->lps = atof(token);
    else if (strcmp(key, "lpr") == 0 || strcmp(key, "lprelease") == 0)
      p->lpr = atof(token);
    else if (strcmp(key, "lpe") == 0 || strcmp(key, "lpenv") == 0)
      p->lpe = atof(token);
    else if (strcmp(key, "hpf") == 0 || strcmp(key, "hcutoff") == 0)
      p->hpf = atof(token);
    else if (strcmp(key, "hpq") == 0 || strcmp(key, "hresonance") == 0)
      p->hpq = atof(token);
    else if (strcmp(key, "hpa") == 0)
      p->hpa = atof(token);
    else if (strcmp(key, "hpd") == 0)
      p->hpd = atof(token);
    else if (strcmp(key, "hps") == 0)
      p->hps = atof(token);
    else if (strcmp(key, "hpr") == 0)
      p->hpr = atof(token);
    else if (strcmp(key, "hpe") == 0 || strcmp(key, "hpenv") == 0)
      p->hpe = atof(token);
    else if (strcmp(key, "bpf") == 0 || strcmp(key, "bandf") == 0)
      p->bpf = atof(token);
    else if (strcmp(key, "bpq") == 0 || strcmp(key, "bandq") == 0)
      p->bpq = atof(token);
    else if (strcmp(key, "bpa") == 0 || strcmp(key, "bpattack") == 0)
      p->bpa = atof(token);
    else if (strcmp(key, "bpd") == 0 || strcmp(key, "bpdecay") == 0)
      p->bpd = atof(token);
    else if (strcmp(key, "bps") == 0 || strcmp(key, "bpsustain") == 0)
      p->bps = atof(token);
    else if (strcmp(key, "bpr") == 0 || strcmp(key, "bprelease") == 0)
      p->bpr = atof(token);
    else if (strcmp(key, "bpe") == 0 || strcmp(key, "bpenv") == 0)
      p->bpe = atof(token);
    else if (strcmp(key, "ftype") == 0)
      p->ftype = get_ftype(token);
    else if (strcmp(key, "gain") == 0)
      p->gain = atof(token);
    else if (strcmp(key, "postgain") == 0)
      p->postgain = atof(token);
    else if (strcmp(key, "velocity") == 0)
      p->velocity = atof(token);
    else if (strcmp(key, "pan") == 0)
      p->pan = atof(token);
    else if (strcmp(key, "reset") == 0)
      p->reset = atoi(token);
    else if (strcmp(key, "attack") == 0)
      p->attack = atof(token);
    else if (strcmp(key, "decay") == 0)
      p->decay = atof(token);
    else if (strcmp(key, "sustain") == 0)
      p->sustain = atof(token);
    else if (strcmp(key, "release") == 0)
      p->release = atof(token);
    else if (strcmp(key, "penv") == 0)
      p->penv = atof(token);
    else if (strcmp(key, "patt") == 0)
      p->patt = atof(token);
    else if (strcmp(key, "pdec") == 0)
      p->pdec = atof(token);
    else if (strcmp(key, "psus") == 0)
      p->psus = atof(token);
    else if (strcmp(key, "prel") == 0)
      p->prel = atof(token);
    else if (strcmp(key, "vib") == 0)
      p->vib = atof(token);
    else if (strcmp(key, "vibmod") == 0)
      p->vibmod = atof(token);
    else if (strcmp(key, "fm") == 0 || strcmp(key, "fmi") == 0)
      p->fm = atof(token);
    else if (strcmp(key, "fmh") == 0)
      p->fmh = atof(token);
    else if (strcmp(key, "fme") == 0)
      p->fme = atof(token);
    else if (strcmp(key, "fma") == 0)
      p->fma = atof(token);
    else if (strcmp(key, "fmd") == 0)
      p->fmd = atof(token);
    else if (strcmp(key, "fms") == 0)
      p->fms = atof(token);
    else if (strcmp(key, "fmr") == 0)
      p->fmr = atof(token);
    else if (strcmp(key, "am") == 0)
      p->am = atof(token);
    else if (strcmp(key, "amdepth") == 0)
      p->amdepth = atof(token);
    else if (strcmp(key, "rm") == 0)
      p->rm = atof(token);
    else if (strcmp(key, "rmdepth") == 0)
      p->rmdepth = atof(token);
    else if (strcmp(key, "phaser") == 0 || strcmp(key, "phaserrate") == 0)
      p->phaser = atof(token);
    else if (strcmp(key, "phaserdepth") == 0)
      p->phaserdepth = atof(token);
    else if (strcmp(key, "phasersweep") == 0)
      p->phasersweep = atof(token);
    else if (strcmp(key, "phasercenter") == 0)
      p->phasercenter = atof(token);
    else if (strcmp(key, "flanger") == 0 || strcmp(key, "flangerrate") == 0)
      p->flanger = atof(token);
    else if (strcmp(key, "flangerdepth") == 0)
      p->flangerdepth = atof(token);
    else if (strcmp(key, "flangerfeedback") == 0)
      p->flangerfeedback = atof(token);
    else if (strcmp(key, "chorus") == 0 || strcmp(key, "chorusrate") == 0)
      p->chorus = atof(token);
    else if (strcmp(key, "chorusdepth") == 0)
      p->chorusdepth = atof(token);
    else if (strcmp(key, "chorusdelay") == 0)
      p->chorusdelay = atof(token);
    else if (strcmp(key, "begin") == 0)
      p->begin = clamp(atof(token), 0, 1);
    else if (strcmp(key, "end") == 0)
      p->end = clamp(atof(token), 0, 1);
    else if (strcmp(key, "coarse") == 0)
      p->coarse = atof(token);
    else if (strcmp(key, "crush") == 0)
      p->crush = atof(token);
    else if (strcmp(key, "distort") == 0)
      p->distort = atof(token);
    else if (strcmp(key, "distortvol") == 0)
      p->distortvol = atof(token);
    else if (strcmp(key, "delay") == 0)
      p->delay = atof(token);
    else if (strcmp(key, "delaytime") == 0)
      p->delaytime = atof(token);
    else if (strcmp(key, "delayfeedback") == 0)
      p->delayfeedback = atof(token);
    else if (strcmp(key, "verb") == 0 || strcmp(key, "reverb") == 0 || strcmp(key, "room") == 0)
      p->verb = atof(token);
    else if (strcmp(key, "verbdecay") == 0 || strcmp(key, "roomdecay") == 0)
      p->verbdecay = atof(token);
    else if (strcmp(key, "verbdamp") == 0 || strcmp(key, "roomdamp") == 0)
      p->verbdamp = atof(token);
    else if (strcmp(key, "verbpredelay") == 0 || strcmp(key, "roompredelay") == 0)
      p->verbpredelay = atof(token);
    else if (strcmp(key, "verbdiff") == 0 || strcmp(key, "roomdiff") == 0)
      p->verbdiff = atof(token);
    else if (strcmp(key, "orbit") == 0)
      p->orbit = atoi(token);

    // ^ dont do anything but assignments here!

    token = strtok(NULL, "/");
    index++;
  }
}

void init_vib(Event *p)
{
  if (isnan(p->vib) && isnan(p->vibmod))
  {
    p->vib_active = false;
    return;
  }
  nan_fallback(&p->vibmod, 0.15);
  nan_fallback(&p->vib, 4);
  p->vib_active = true;
}

void init_fm(Event *p)
{
  if (isnan(p->fm) && isnan(p->fmh))
    return;
  nan_fallback(&p->fm, 1.0);
  nan_fallback(&p->fmh, 1.0);
  p->fm_active = true;
}

void init_am(Event *p)
{
  if (isnan(p->am) && isnan(p->amdepth))
  {
    p->am_active = false;
    return;
  }
  p->am_active = true;
  nan_fallback(&p->am, 4.0);
  nan_fallback(&p->amdepth, 0.5);
}

void init_rm(Event *p)
{
  if (isnan(p->rm) && isnan(p->rmdepth))
  {
    p->rm_active = false;
    return;
  }
  p->rm_active = true;
  nan_fallback(&p->rmdepth, 1.0);
}

void init_phaser(Event *p)
{
  if (isnan(p->phaser) && isnan(p->phaserdepth) &&
      isnan(p->phasersweep) && isnan(p->phasercenter))
  {
    p->phaser_active = false;
    return;
  }
  p->phaser_active = true;
  nan_fallback(&p->phaser, 1.0);
  nan_fallback(&p->phaserdepth, 0.75);
  nan_fallback(&p->phasersweep, 2000.0);
  nan_fallback(&p->phasercenter, 1000.0);
}

void init_flanger(Event *p)
{
  if (isnan(p->flanger) && isnan(p->flangerdepth) && isnan(p->flangerfeedback))
  {
    p->flanger_active = false;
    return;
  }
  p->flanger_active = true;
  nan_fallback(&p->flanger, 0.5);
  nan_fallback(&p->flangerdepth, 0.5);
  nan_fallback(&p->flangerfeedback, 0.5);
}

void init_chorus(Event *p)
{
  if (isnan(p->chorus) && isnan(p->chorusdepth) && isnan(p->chorusdelay))
  {
    p->chorus_active = false;
    return;
  }
  p->chorus_active = true;
  nan_fallback(&p->chorus, 0.5);
  nan_fallback(&p->chorusdepth, 0.5);
  nan_fallback(&p->chorusdelay, 25.0);
}

void init_lp(Event *p)
{
  if (isnan(p->lpe) && isnan(p->lpf))
    return;
  nan_fallback(&p->lpf, 20000);
  nan_fallback(&p->lpq, 0.2);
  p->lp_active = true;
}
void init_hp(Event *p)
{
  if (isnan(p->hpe) && isnan(p->hpf))
    return;
  nan_fallback(&p->hpf, 0);
  nan_fallback(&p->hpq, 0.2);
  p->hp_active = true;
}
void init_bp(Event *p)
{
  if (isnan(p->bpe) && isnan(p->bpf))
    return;
  nan_fallback(&p->bpf, 20000);
  nan_fallback(&p->bpq, 0.2);
  p->bp_active = true;
}
void init_distort(Event *p)
{
  if (isnan(p->distort))
    return;
  nan_fallback(&p->distortvol, 1);
  p->distort_active = true;
}
void init_delay(Event *p)
{
  if (isnan(p->delay) && isnan(p->delaytime) && isnan(p->delayfeedback))
    return;

  nan_fallback(&p->delay, 0.5);
  nan_fallback(&p->delaytime, 0.333);
  nan_fallback(&p->delayfeedback, 0.6);
  p->delay_active = true;
}

void init_verb(Event *p)
{
  if (isnan(p->verb) && isnan(p->verbdecay) && isnan(p->verbdamp) && isnan(p->verbpredelay) && isnan(p->verbdiff))
    return;

  nan_fallback(&p->verb, 0.5);
  nan_fallback(&p->verbdecay, 0.75);
  nan_fallback(&p->verbdamp, 0.95);
  nan_fallback(&p->verbpredelay, 0.1);
  nan_fallback(&p->verbdiff, 0.7);
  p->verb_active = true;
}

void init_event(Event *p, Engine *engine)
{
  // make sure no NAN is left
  p->channels = 1;
  p->rate = p->freq; // we dont touch freq so init_event can run multiple times
  if (p->file != -1)
  {
    if (p->file >= 0 && p->file < engine->max_files)
      p->file_source = engine->files[p->file];
    else
    {
#if DOUGH_LOG
      printf("init_event: file %d out of bounds. ignoring\n", p->file);
#endif // DOUGH_LOG
    }
  }

  if (p->file_source.pcm != NULL)
  {
    p->sound = FILE_SRC;
    nan_fallback(&p->begin, 0.0);
    nan_fallback(&p->end, 1.0);
    p->channels = p->file_source.channels; // TODO: fix stereo fx first
    p->rate = isnan(p->rate) ? 1 : p->rate / p->file_source.freq;

    int slice_frames = (int)((p->end - p->begin) * p->file_source.frames);
    if (slice_frames != 0)
      p->rate *= engine->sr / slice_frames;
  }
  else if (p->sound == UNKNOWN_SOURCE)
    p->sound = TRI_OSC;
  nan_fallback(&p->gate, 1.0);
  nan_fallback(&p->gain, 1.0);
  nan_fallback(&p->velocity, 1.0);
  nan_fallback(&p->postgain, 1.0);
  // nan_fallback(&p->freq, 60.0); // 60Hz hum (MSG shoutout)
  nan_fallback(&p->rate, 330.0);
  nan_fallback(&p->speed, 1.0);
  nan_fallback(&p->gate, 1.0);
  nan_fallback(&p->pan, 0.5);
  nan_fallback(&p->pw, 0.5);
  init_vib(p);
  init_fm(p);
  init_am(p);
  init_rm(p);
  init_phaser(p);
  init_flanger(p);
  init_chorus(p);
  init_lp(p);
  init_hp(p);
  init_bp(p);
  init_distort(p);
  init_delay(p);
  init_verb(p);
  if (!isnan(p->coarse))
    p->coarse_active = true;
  if (!isnan(p->crush))
    p->crush_active = true;
  if (!isnan(p->glide))
    p->glide_active = true;
  if (p->ftype == -1)
    p->ftype = 0;
  init_envelope(&p->lpe, &p->lpa, &p->lpd, &p->lps, &p->lpr, &p->lp_adsr_active);
  init_envelope(&p->hpe, &p->hpa, &p->hpd, &p->hps, &p->hpr, &p->hp_adsr_active);
  init_envelope(&p->bpe, &p->bpa, &p->bpd, &p->bps, &p->bpr, &p->bp_adsr_active);
  init_envelope(&p->penv, &p->patt, &p->pdec, &p->psus, &p->prel, &p->p_adsr_active);
  init_envelope(&p->gain, &p->attack, &p->decay, &p->sustain, &p->release, &p->gain_adsr_active);
  init_envelope(&p->fme, &p->fma, &p->fmd, &p->fms, &p->fmr, &p->fm_adsr_active);
  p->gain_adsr_active = true; // always apply gain envelope
}

// ---
// voice system
// ---

typedef struct
{
  BiquadFilter lp_bq[4];
  BiquadFilter hp_bq[4];
  BiquadFilter bp_bq[4];
  Coarse coarse;
  Phaser phaser;
  Flanger flanger;
} ChannelFX;

typedef struct
{
  Lag glide_lag;
  Phasor phasor;
  PinkNoise noise_pink;
  BrownNoise noise_brown;
  ADSRNode gain_adsr;
  ADSRNode lp_adsr;
  ADSRNode hp_adsr;
  ADSRNode bp_adsr;
  ADSRNode fm_adsr;
  ADSRNode p_adsr;
  Phasor vib_lfo;
  Phasor fm_modulator;
  Phasor am_lfo;
  Phasor rm_lfo;
  Chorus chorus;
  uint64_t id; // voice_counter at play_event time
  float time;
  float ch[CHANNELS]; // audio channels
  ChannelFX fx[CHANNELS];
  Event p;
} voice;

typedef struct
{
  Delay delay[CHANNELS];
  float delaysend[CHANNELS];
  float delaytime;
  float delayfeedback;

  DattorroVerb verb;
  float verbsend[CHANNELS];
  float verbdecay;
  float verbdamp;
  float verbpredelay;
  float verbdiff;
} Orbit;

int allocate_voice(Engine *engine)
{
  if (engine->active_voices >= engine->max_voices)
    return -1; // no free voice, todo: implement voice stealing
  return engine->active_voices++;
}

void free_voice(Engine *engine, int i)
{
  voice *voices = (voice *)engine->memory;
  voices[i] = voices[--engine->active_voices];
}

void reset_voice(voice *v)
{
  for (int c = 0; c < CHANNELS; c++)
  {
    Phaser_init(&v->fx[c].phaser);
    Flanger_init(&v->fx[c].flanger);
    Coarse_init(&v->fx[c].coarse);
    for (int i = 0; i < 4; i++)
    {
      BiquadFilter_init(&v->fx[c].lp_bq[i]);
      BiquadFilter_init(&v->fx[c].hp_bq[i]);
      BiquadFilter_init(&v->fx[c].bp_bq[i]);
    }
  }
  ADSRNode_init(&v->gain_adsr);
  ADSRNode_init(&v->lp_adsr);
  ADSRNode_init(&v->hp_adsr);
  ADSRNode_init(&v->bp_adsr);
  ADSRNode_init(&v->fm_adsr);
  ADSRNode_init(&v->p_adsr);
  Phasor_init(&v->phasor);
  PinkNoise_init(&v->noise_pink);
  BrownNoise_init(&v->noise_brown);
  Phasor_init(&v->vib_lfo);
  Phasor_init(&v->fm_modulator);
  Phasor_init(&v->am_lfo);
  Phasor_init(&v->rm_lfo);
  Chorus_init(&v->chorus);
  Lag_init(&v->glide_lag);
  v->time = 0.0;
}

// sources

bool run_source(voice *v, float freq)
{
  switch (v->p.sound)
  {
  case TRI_OSC:
    v->ch[0] = TriOsc_update(&v->phasor, freq) * GAIN_TRI;
    return false;
  case SINE_OSC:
    v->ch[0] = SineOsc_update(&v->phasor, freq) * GAIN_SINE;
    return false;
  case ZAW_OSC:
    v->ch[0] = ZawOsc_update(&v->phasor, freq) * GAIN_SAW;
    return false;
  case SAW_OSC:
    v->ch[0] = SawOsc_update(&v->phasor, freq) * GAIN_SAW;
    return false;
  case PULSE_OSC:
    v->ch[0] = PulseOsc_update(&v->phasor, freq, v->p.pw) * GAIN_PULSE;
    return false;
  case PULZE_OSC:
    v->ch[0] = PulzeOsc_update(&v->phasor, freq, v->p.pw) * GAIN_PULSE;
    return false;
  case WHITE_NOISE:
    v->ch[0] = (random_float() * 2 - 1) * GAIN_WHITE;
    return false;
  case PINK_NOISE:
    v->ch[0] = PinkNoise_update(&v->noise_pink) * GAIN_PINK;
    return false;
  case BROWN_NOISE:
    v->ch[0] = BrownNoise_update(&v->noise_brown) * GAIN_BROWN;
    return false;
  case CONST:
    v->ch[0] = 1;
    return false;
  case FILE_SRC:
    return FileSource_update(&v->phasor, freq, &v->p.file_source, v->p.begin, v->p.end, v->ch);
  case UNKNOWN_SOURCE:
    return false;
  }
  return false;
}

// fx

// pitch modulation

void apply_penv(float *freq, voice *v)
{
  // float gate = v->gate;
  float gate = 1.0; // never enter pitch release phase
  float env = ADSRNode_update(&v->p_adsr, v->time, gate, v->p.patt, v->p.pdec, v->p.psus, v->p.prel);
  if (v->p.psus == 1) // attack
    env -= 1;         // env [-1, 0]
  // ^ this makes sure the attack targets freq
  float factor = our_exp2f((env * v->p.penv) / 12);
  // ^ convert env from semitones to frequency ratio
  *freq = *freq * factor;
}

void apply_vib(float *freq, voice *v)
{
  float mod = SineOsc_update(&v->vib_lfo, v->p.vib);
  *freq = *freq * (our_exp2f(mod * v->p.vibmod / 12));
}

void apply_fm(float *freq, voice *v)
{

  float fm = v->p.fm;
  if (v->p.fm_adsr_active)
  {
    float env = ADSRNode_update(&v->fm_adsr, v->time, v->p.gate, v->p.fma, v->p.fmd, v->p.fms, v->p.fmr);
    fm = v->p.fme * env * fm + fm;
  }
  float modfreq = *freq * v->p.fmh;
  float modgain = modfreq * fm;
  float mod = SineOsc_update(&v->fm_modulator, modfreq);
  *freq = *freq + mod * modgain;
}

void apply_lag(Lag *lag, float source, float *target, float rate)
{
  *target = Lag_update(lag, source, rate);
}

void run_voice(Engine *engine, int i)
{
  voice *voices = (voice *)engine->memory;
  voice *v = &voices[i];
  float gain_envelope = 0.0;
  if (v->p.gain_adsr_active)
    gain_envelope = ADSRNode_update(&v->gain_adsr, v->time, v->p.gate, v->p.attack, v->p.decay, v->p.sustain, v->p.release);
  if (v->gain_adsr.state == ADSR_OFF)
    return free_voice(engine, i); // free + short-circuit when envelope is over

  float freq = v->p.rate;
  freq *= v->p.speed;

  if (v->p.glide_active)
    freq = Lag_update(&v->glide_lag, freq, v->p.glide);
  if (v->p.fm_active)
    apply_fm(&freq, v);
  if (v->p.p_adsr_active)
    apply_penv(&freq, v);
  if (v->p.vib_active)
    apply_vib(&freq, v);

  // run source, fill v->ch
  bool wrapped = run_source(v, freq);
  if (wrapped)
    return free_voice(engine, i);

  // update things that are not per channel (like envelopes):

  // low pass envelope
  float lp_cutoff = v->p.lpf;
  if (v->p.lp_active)
  {
    if (v->p.lp_adsr_active)
    {
      float env = ADSRNode_update(&v->lp_adsr, v->time, v->p.gate, v->p.lpa, v->p.lpd, v->p.lps, v->p.lpr);
      lp_cutoff = clamp(v->p.lpe * env * lp_cutoff + lp_cutoff, 0, 20000);
    }
  }

  // high pass envelope
  float hp_cutoff = v->p.hpf;
  if (v->p.hp_active)
  {
    if (v->p.hp_adsr_active)
    {
      float env = ADSRNode_update(&v->hp_adsr, v->time, v->p.gate, v->p.hpa, v->p.hpd, v->p.hps, v->p.hpr);
      hp_cutoff = clamp(v->p.hpe * env * hp_cutoff + hp_cutoff, 0, 20000);
    }
  }

  // band pass envelope
  float bp_cutoff = v->p.bpf;
  if (v->p.bp_active)
  {
    if (v->p.bp_adsr_active)
    {
      float env = ADSRNode_update(&v->bp_adsr, v->time, v->p.gate, v->p.bpa, v->p.bpd, v->p.bps, v->p.bpr);
      bp_cutoff = clamp(v->p.bpe * env * bp_cutoff + bp_cutoff, 0, 20000);
    }
  }

  // lfo

  float am_gain = 1.0f;
  if (v->p.am_active)
  {
    float mod = SineOsc_update(&v->am_lfo, v->p.am);
    float depth = clamp(v->p.amdepth, 0.0, 1.0);
    am_gain = 1.0 + mod * depth;
  }
  float rm_gain = 1.0f;
  if (v->p.rm_active)
  {
    float mod = SineOsc_update(&v->rm_lfo, v->p.rm);
    float depth = clamp(v->p.rmdepth, 0.0, 1.0);
    rm_gain = (1.0 - depth) + mod * depth;
  }

  // run channel wise stuff

  for (int i = 0; i < v->p.channels; i++)
  {
    // pre-fx gain
    v->ch[i] *= v->p.gain * v->p.velocity;
    // apply filters
    if (v->p.lp_active)
    {
      int num_stages = (v->p.ftype == 0) ? 1 : (v->p.ftype == 1) ? 2
                                                                 : 4;
      float signal = v->ch[i];
      for (int stage = 0; stage < num_stages; stage++)
      {
        signal = BiquadFilter_update(&v->fx[i].lp_bq[stage], signal, 0, lp_cutoff, v->p.lpq, 0);
      }
      v->ch[i] = signal;
    }
    if (v->p.hp_active)
    {
      int num_stages = (v->p.ftype == 0) ? 1 : (v->p.ftype == 1) ? 2
                                                                 : 4;
      float signal = v->ch[i];
      for (int stage = 0; stage < num_stages; stage++)
      {
        signal = BiquadFilter_update(&v->fx[i].hp_bq[stage], signal, 1, hp_cutoff, v->p.hpq, 0);
      }
      v->ch[i] = signal;
    }
    if (v->p.bp_active)
    {
      int num_stages = (v->p.ftype == 0) ? 1 : (v->p.ftype == 1) ? 2
                                                                 : 4;
      float signal = v->ch[i];
      for (int stage = 0; stage < num_stages; stage++)
      {
        signal = BiquadFilter_update(&v->fx[i].bp_bq[stage], signal, 2, bp_cutoff, v->p.bpq, 0);
      }
      v->ch[i] = signal;
    }

    // apply other fx
    if (v->p.coarse_active)
      v->ch[i] = Coarse_update(&v->fx[i].coarse, v->ch[i], v->p.coarse);
    if (v->p.crush_active)
      v->ch[i] = crush(v->ch[i], v->p.crush);
    if (v->p.distort_active)
      v->ch[i] = distort(v->ch[i], v->p.distort, v->p.distortvol);
    // amplitude modulation
    if (v->p.am_active)
      v->ch[i] *= am_gain;
    // ring modulation
    if (v->p.rm_active)
      v->ch[i] *= rm_gain;
    // phaser
    if (v->p.phaser_active)
    {
      v->ch[i] = Phaser_update(&v->fx[i].phaser, v->ch[i], v->p.phaser,
                               v->p.phaserdepth, v->p.phasercenter,
                               v->p.phasersweep);
    }
    // flanger
    if (v->p.flanger_active)
    {
      v->ch[i] = Flanger_update(&v->fx[i].flanger, v->ch[i], v->p.flanger,
                                v->p.flangerdepth, v->p.flangerfeedback);
    }
    v->ch[i] *= gain_envelope * v->p.postgain;
  }
  // fill up remaining channels
  for (int i = v->p.channels; i < CHANNELS; i++)
    v->ch[i] = v->ch[i % v->p.channels];

  // apply chorus (stereo effect)
  if (v->p.chorus_active)
  {
    Chorus_update(&v->chorus, &v->ch[0], &v->ch[1], v->p.chorus,
                  v->p.chorusdepth, v->p.chorusdelay);
  }

  // apply panning
  if (v->p.pan != 0.5)
  {
    float panpos = (v->p.pan * fPI) / 2.0;
    v->ch[0] *= par_cosf(panpos);
    v->ch[1] *= par_sinf(panpos);
  }
  // advance local voice time
  v->time += 1 / engine->sr;
  // release when duration is over. duration 0 = infinite
  if (!isnan(v->p.duration) && v->p.duration != 0 && v->time > v->p.duration)
    v->p.gate = 0.0;
}

// ---
// api surface
// ---

Event p;
float output[BLOCK_SIZE * CHANNELS];

double doughtime = 0.0; // global time in seconds
int64_t tick = 0;       // current sample index
double get_time()
{
  return doughtime;
}

#if DOUGH_LOG

void log_event(Event *e)
{
  if (!isnan(e->time))
    printf("time %f \n", e->time);
  if (!isnan(e->gate))
    printf("gate %f \n", e->gate);
  if (!isnan(e->duration))
    printf("duration %f \n", e->duration);
  if (e->group != -1)
    printf("group %d \n", e->group);
  if (e->voices != -1)
    printf("voices %d \n", e->voices);
  if (!isnan(e->freq))
    printf("freq %f \n", e->freq);
  if (!isnan(e->glide))
    printf("glide %f \n", e->glide);
  if (e->sound != -1)
    printf("sound %d \n", e->sound);
  if (e->file_source.pcm != NULL)
    printf("file_source %p frames %d chan %d\n", e->file_source.pcm, e->file_source.frames, e->file_source.channels);
  if (!isnan(e->pw))
    printf("pw %f \n", e->pw);
  if (!isnan(e->lpf))
    printf("lpf %f \n", e->lpf);
  if (!isnan(e->lpq))
    printf("lpq %f \n", e->lpq);
  if (!isnan(e->lpa))
    printf("lpa %f \n", e->lpa);
  if (!isnan(e->lpd))
    printf("lpd %f \n", e->lpd);
  if (!isnan(e->lps))
    printf("lps %f \n", e->lps);
  if (!isnan(e->lpr))
    printf("lpr %f \n", e->lpr);
  if (!isnan(e->lpe))
    printf("lpe %f \n", e->lpe);
  if (!isnan(e->hpf))
    printf("hpf %f \n", e->hpf);
  if (!isnan(e->hpq))
    printf("hpq %f \n", e->hpq);
  if (!isnan(e->hpd))
    printf("hpd %f \n", e->hpd);
  if (!isnan(e->hps))
    printf("hps %f \n", e->hps);
  if (!isnan(e->hpr))
    printf("hpr %f \n", e->hpr);
  if (!isnan(e->hpe))
    printf("hpe %f \n", e->hpe);
  if (!isnan(e->bpf))
    printf("bpf %f \n", e->bpf);
  if (!isnan(e->bpq))
    printf("bpq %f \n", e->bpq);
  if (!isnan(e->bpd))
    printf("bpd %f \n", e->bpd);
  if (!isnan(e->bps))
    printf("bps %f \n", e->bps);
  if (!isnan(e->bpr))
    printf("bpr %f \n", e->bpr);
  if (!isnan(e->bpe))
    printf("bpe %f \n", e->bpe);
  if (!isnan(e->gain))
    printf("gain %f \n", e->gain);
  if (!isnan(e->pan))
    printf("pan %f \n", e->pan);
  if (e->reset != -1)
    printf("reset %d \n", e->reset);
  if (!isnan(e->attack))
    printf("attack %f \n", e->attack);
  if (!isnan(e->decay))
    printf("decay %f \n", e->decay);
  if (!isnan(e->sustain))
    printf("sustain %f \n", e->sustain);
  if (!isnan(e->release))
    printf("release %f \n", e->release);
  if (!isnan(e->penv))
    printf("penv %f \n", e->penv);
  if (!isnan(e->patt))
    printf("patt %f \n", e->patt);
  if (!isnan(e->pdec))
    printf("pdec %f \n", e->pdec);
  if (!isnan(e->psus))
    printf("psus %f \n", e->psus);
  if (!isnan(e->prel))
    printf("prel %f \n", e->prel);
  if (!isnan(e->vib))
    printf("vib %f \n", e->vib);
  if (!isnan(e->vibmod))
    printf("vibmod %f \n", e->vibmod);
  if (!isnan(e->fm))
    printf("fm %f \n", e->fm);
  if (!isnan(e->fmh))
    printf("fmh %f \n", e->fmh);
  if (!isnan(e->crush))
    printf("crush %f \n", e->crush);
  if (!isnan(e->coarse))
    printf("coarse %f \n", e->coarse);
  if (!isnan(e->distort))
    printf("distort %f \n", e->distort);
  if (!isnan(e->distortvol))
    printf("distortvol %f \n", e->distortvol);
  if (!isnan(e->delay))
    printf("delay %f \n", e->delay);
  if (!isnan(e->delaytime))
    printf("delaytime %f \n", e->delaytime);
  if (!isnan(e->delayfeedback))
    printf("delayfeedback %f \n", e->delayfeedback);
  if (!isnan(e->verb))
    printf("verb %f \n", e->verb);
  if (!isnan(e->verbdecay))
    printf("verbdecay %f \n", e->verbdecay);
  if (!isnan(e->verbdamp))
    printf("verbdamp %f \n", e->verbdamp);
  if (!isnan(e->verbpredelay))
    printf("verbpredelay %f \n", e->verbpredelay);
  if (!isnan(e->verbdiff))
    printf("verbdiff %f \n", e->verbdiff);
  if (e->orbit != -1)
    printf("orbit %d \n", e->orbit);
  printf("\n");
}

#endif // DOUGH_LOG

typedef struct
{
  int count;
  int oldest;
} GroupState;

GroupState get_group_state(Engine *engine, int group)
{
  voice *voices = (voice *)engine->memory;
  int count = 0;
  uint64_t oldest_id;
  int oldest = -1;
  for (int v = 0; v < engine->active_voices; v++)
  {
    voice *voice = &voices[v];
    if (voice->p.group == group)
    {
      count++;
      if (oldest == -1 || voice->id < oldest_id)
      {
        oldest_id = voice->id;
        oldest = v;
      }
    }
  }
  return (GroupState){count, oldest};
}

void shift_envelopes(voice *v, float offset)
{
  v->gain_adsr.startTime += offset;
  v->lp_adsr.startTime += offset;
  v->hp_adsr.startTime += offset;
  v->bp_adsr.startTime += offset;
  v->fm_adsr.startTime += offset;
  v->p_adsr.startTime += offset;
}

int play_event(Engine *engine, Event *p)
{
  int i;
  bool voice_stolen = false;
  if (p->group != -1 && p->voices != -1)
  {
    // steal voice from group if group is full
    GroupState g = get_group_state(engine, p->group);
    voice_stolen = g.count >= p->voices;
    i = !voice_stolen ? allocate_voice(engine) : g.oldest;
  }
  else
    i = allocate_voice(engine);
  if (i == -1)
    return -1; // no free voice...

  voice *voices = (voice *)engine->memory;
  voice *v = &voices[i];

  bool is_fresh = !voice_stolen || p->reset == 1;
  if (is_fresh)
    reset_voice(v); // reset if not (yet) active

  v->p = *p; // write event params to voice
  v->id = engine->voice_counter++;
  // the following is important for retriggered voices:
  shift_envelopes(v, -v->time); // translates envelope time
  v->time = 0;                  // reset time

#if DOUGH_LOG
  log_event(&v->p); // log before init to only log explcitly set values
#endif              // DOUGH_LOG

  init_event(&v->p, engine);

  if (is_fresh)
  {
    // prevent glide from 0
    v->glide_lag.s = v->p.rate;
    // avoid early "wrap"
    v->phasor.phase = v->p.speed < 0 ? 1.0 : 0.0;
  }

  return i;
}

// swapback array for scheduling, thanks nic barker: https://youtu.be/WwkuAqObplU?si=9zb07f7UqNyPComH&t=2774
typedef struct
{
  Event *events;
  int size;
} Schedule;
Schedule schedule = {0};

void run_event(Engine *engine, Event *p);

// add element to end of array
void schedule_push(Schedule *a, Event *e)
{
  if (a->size >= engine.max_events)
    return; // we are at capacity
  a->events[a->size] = *e;
  a->size++;
}
// remove element at index
void schedule_remove(Schedule *a, int index)
{
  a->events[index] = a->events[a->size - 1];
  a->size--;
}
// process due events
void schedule_update(Schedule *a, double time)
{
  // we need to go backwards to avoid skipping swapped elements...
  for (int i = a->size - 1; i >= 0; i--)
  {
    if (a->events[i].time <= time)
    {
      double diff = time - a->events[i].time;
      if (diff < 0.01)
        run_event(&engine, &a->events[i]);
#if DOUGH_LOG
      else
        printf("drop event: %.1f ms too late\n", diff * 1000);
#endif // DOUGH_LOG
      // ^ only play event if it's recently due (<10ms)
      if (isnan(a->events[i].repeat) || a->events[i].repeat <= 0)
        schedule_remove(a, i);
      else
      {
        // event repeats, so we need to find the next time in the future
        double diff = time - a->events[i].time;
        int skip = floor(diff / a->events[i].repeat) + 1;
        a->events[i].time += skip * (double)a->events[i].repeat;
      }
    }
  }
}

void reset_time()
{
  tick = 0;
  doughtime = 0.0;
}

void reset_schedule()
{
  schedule.size = 0;
}

void reset()
{
  reset_time();
  reset_schedule();
}

// current framebuffer frame index
int frame = 0;

void gen_sample(Engine *engine, float *output, int i)
{
  for (int c = 0; c < CHANNELS; c++)
    output[i * CHANNELS + c] = 0.0;
  voice *voices = (voice *)engine->memory;
  Orbit *orbits = (Orbit *)(engine->memory + engine->max_voices * sizeof(voice));
  for (int v = 0; v < engine->active_voices; v++)
  {
    voice *voice = &voices[v];
    run_voice(engine, v);
    for (int c = 0; c < CHANNELS; c++)
    {
      output[i * CHANNELS + c] += voice->ch[c];
      if (voice->p.delay_active)
      {
        int o = voice->p.orbit % engine->max_orbits;
        orbits[o].delaytime = voice->p.delaytime;
        orbits[o].delayfeedback = voice->p.delayfeedback;
        // ^ this could be done at voice->init time
        orbits[o].delaysend[c] += voice->ch[c] * voice->p.delay;
      }
      if (voice->p.verb_active)
      {
        int o = voice->p.orbit % engine->max_orbits;
        orbits[o].verbdecay = voice->p.verbdecay;
        orbits[o].verbdamp = voice->p.verbdamp;
        orbits[o].verbpredelay = voice->p.verbpredelay;
        orbits[o].verbdiff = voice->p.verbdiff;
        orbits[o].verbsend[c] += voice->ch[c] * voice->p.verb;
      }
    }
  }
  // Process delay (per channel, per orbit)
  for (int c = 0; c < CHANNELS; c++)
  {
    for (int o = 0; o < engine->max_orbits; o++)
    {
      // flush to zero to prevent endless number crunching
      orbits[o].delaysend[c] = ftz(orbits[o].delaysend[c], 0.0001);
      // TODO: only update active orbits
      float delayout = Delay_update(&orbits[o].delay[c], orbits[o].delaysend[c], orbits[o].delaytime, engine->max_delay_samples);
      orbits[o].delaysend[c] = delayout * orbits[o].delayfeedback;
      output[i * CHANNELS + c] += delayout;
    }
  }

  // Process reverb (per orbit, stereo)
  for (int o = 0; o < engine->max_orbits; o++)
  {
    // Mix stereo to mono for reverb input
    float mono_in = (orbits[o].verbsend[0] + orbits[o].verbsend[1]) * 0.5f;
    mono_in = ftz(mono_in, 0.0001f);

    if (mono_in != 0.0f)
    {
      // Update reverb parameters (only when there's input for efficiency)
      DattorroVerb_setDecay(&orbits[o].verb, orbits[o].verbdecay);
      DattorroVerb_setDamping(&orbits[o].verb, orbits[o].verbdamp);
      DattorroVerb_setPreDelay(&orbits[o].verb, orbits[o].verbpredelay);
      DattorroVerb_setInputDiffusion1(&orbits[o].verb, orbits[o].verbdiff);
      DattorroVerb_setInputDiffusion2(&orbits[o].verb, orbits[o].verbdiff);
    }

    DattorroVerb_process(&orbits[o].verb, mono_in);

    // Get stereo output and add to output channels
    float verb_left = DattorroVerb_getLeft(&orbits[o].verb);
    float verb_right = DattorroVerb_getRight(&orbits[o].verb);
    output[i * CHANNELS + 0] += verb_left;
    output[i * CHANNELS + 1] += verb_right;

    // Clear send accumulators
    orbits[o].verbsend[0] = 0.0f;
    orbits[o].verbsend[1] = 0.0f;
  }

  // Apply final gain and clamp
  for (int c = 0; c < CHANNELS; c++)
  {
    output[i * CHANNELS + c] = clamp(output[i * CHANNELS + c] * GAIN_MIX, -1, 1);
    engine->framebuffer[frame] = output[i * CHANNELS + c];
    frame++;
    if (frame > engine->framebuffer_length)
      frame -= engine->framebuffer_length;
  }
}

void dough_render(float *out, int frames, double start_time)
{
  for (int i = 0; i < frames; i++)
  {
    double time = start_time + i / (double)engine.sr;
    schedule_update(&schedule, time);
    gen_sample(&engine, out, i);
  }
}

void dsp()
{
  dough_render(output, BLOCK_SIZE, doughtime);
  tick += BLOCK_SIZE;
  doughtime = tick / (double)engine.sr;
}

void update_all(Engine *engine, Event *p)
{
  for (int i = 0; i < engine->max_voices; i++)
  {
    voice *voices = (voice *)engine->memory;
    voice *v = &voices[i];
    merge_event(p, &v->p);
    init_event(&v->p, engine);
  }
}

void update_group(Engine *engine, Event *p)
{
  for (int i = 0; i < engine->max_voices; i++)
  {
    voice *voices = (voice *)engine->memory;
    voice *v = &voices[i];
    if (p->group != -1 && v->p.group == p->group)
    {
      // TODO: filter out inactive voices?
      merge_event(p, &v->p);
      init_event(&v->p, engine);
    }
  }
}

int release(Engine *engine, int i)
{
  voice *voices = (voice *)engine->memory;
  voice *v = &voices[i];
  v->p.gate = 0.0;
  return i;
}

void panic(Engine *engine)
{
  engine->active_voices = 0;
}

void hush(Engine *engine)
{
  for (int i = 0; i < engine->max_voices; i++)
    release(engine, i);
}

// hush only sounds that are endless (no duration)
void hush_endless(Engine *engine)
{
  voice *voices = (voice *)engine->memory;
  for (int i = 0; i < engine->max_voices; i++)
  {
    if (isnan(voices[i].p.duration))
      release(engine, i);
  }
}

void hush_group(Engine *engine, int group)
{
  voice *voices = (voice *)engine->memory;
  for (int i = 0; i < engine->max_voices; i++)
  {
    if (voices[i].p.group != -1 && voices[i].p.group == group)
      release(engine, i);
  }
}

void run_event(Engine *engine, Event *p)
{
  if (p->cmd == CMD_PLAY)
  {
    play_event(engine, p);
  }
  else if (p->cmd == CMD_UPDATE_ALL)
  {
    update_all(engine, p);
  }
  else if (p->cmd == CMD_UPDATE_GROUP)
  {
    update_group(engine, p);
  }
  else if (p->cmd == CMD_PANIC)
  {
    panic(engine);
  }
  else if (p->cmd == CMD_HUSH)
  {
    hush(engine);
  }
  else if (p->cmd == CMD_HUSH_ENDLESS)
  {
    hush_endless(engine);
  }
  else if (p->cmd == CMD_HUSH_GROUP)
  {
    hush_group(engine, p->group);
  }
  else if (p->cmd == CMD_RESET)
  {
    panic(engine);
    reset();
  }
  else if (p->cmd == CMD_RESET_TIME)
  {
    reset_time();
  }
  else if (p->cmd == CMD_RESET_SCHEDULE)
  {
    reset_schedule();
  }
}

void handle_event(Engine *engine, Event *p, Schedule *schedule)
{
  if (isnan(p->time))
    run_event(engine, p);
  else
    schedule_push(schedule, p);
}

void dough_evaluate(const char *msg)
{
  reset_event(&p); // clear previous read
  parse_event(&p, msg);
  handle_event(&engine, &p, &schedule);
}

void evaluate_input()
{
  dough_evaluate(event_input);
}

// ---
// engine setup
// ---
void dough_engine_init(Engine *engine, int sample_rate, int max_voices, int max_orbits, int max_delay_time, int max_pcm, int max_events, int max_files)
{
  // store params in the engine
  engine->sr = (float)sample_rate;
  engine->isr = 1.0 / engine->sr;
  engine->lag_unit = sample_rate / 10;
  engine->max_voices = max_voices;
  engine->active_voices = 0;
  engine->max_orbits = max_orbits;
  engine->max_events = max_events;
  engine->max_files = max_files;
  engine->max_delay_samples = max_delay_time * sample_rate;
  engine->framebuffer_length = floor(sample_rate / 60.0 * CHANNELS) * 4;
  engine->context_offset = 0.0;
  engine->voice_counter = 0;

  // compute the memory size
  int vs = max_voices * sizeof(voice);
  int os = max_orbits * sizeof(Orbit);
  int delay_size = engine->max_delay_samples * sizeof(float);
  int ds = max_orbits * CHANNELS * delay_size;
  int verb_size = DattorroVerb_getMemorySize() * sizeof(float);
  int vbs = max_orbits * verb_size;
  int es = max_events * sizeof(Event);
  int fs = max_pcm * sizeof(float);
  int fb = engine->framebuffer_length * sizeof(float);
  int files = max_files * sizeof(FileSource);

  // allocate dough memory
  int total_size = vs + os + ds + vbs + es + fs + fb + files;
  engine->memory = malloc(total_size);

  char *head = engine->memory + vs;
  // assign dangling pointers
  Orbit *orbits = (Orbit *)head;
  head += os;

  for (int o = 0; o < engine->max_orbits; o++)
  {
    for (int c = 0; c < CHANNELS; c++)
    {
      orbits[o].delay[c].buffer = (float *)head;
      head += delay_size;
    }
  }

  // assign verb buffers and initialize
  for (int o = 0; o < engine->max_orbits; o++)
  {
    DattorroVerb_init(&orbits[o].verb, (float *)head);
    orbits[o].verbsend[0] = 0.0f;
    orbits[o].verbsend[1] = 0.0f;
    orbits[o].verbdecay = 0.75f;
    orbits[o].verbdamp = 0.95f;
    orbits[o].verbpredelay = 0.1f;
    orbits[o].verbdiff = 0.7f;
    head += verb_size;
  }

  schedule.events = (Event *)head;
  head += es;
  engine->files = (FileSource *)head;
  for (int f = 0; f < engine->max_files; f++)
    engine->files[f].pcm = NULL;
  head += files;

  engine->pcm = (float *)head;
  engine->pcm_offset = 0;
  engine->max_pcm = max_pcm;
  head += fs;
  engine->framebuffer = (float *)head;
  head += fb;

  if (head != engine->memory + total_size)
  {
#if DOUGH_LOG
    printf("engine memory mismatch!\n");
#endif // DOUGH_LOG
  }

  char out[256];
  char tmp[256];
  out[0] = '\0';

#ifdef CLANGWASM
  strcat(out, "?vs=");
  strcat(out, itoa_u((uintptr_t)engine->memory, tmp));
  strcat(out, "&pcm=");
  strcat(out, itoa_u((uintptr_t)engine->pcm, tmp));
  strcat(out, "&framebuffer=");
  strcat(out, itoa_u((uintptr_t)engine->framebuffer, tmp));
  strcat(out, "&framebuffer_length=");
  strcat(out, itoa_u(engine->framebuffer_length, tmp));
  strcat(out, "&frame=");
  strcat(out, itoa_u((uintptr_t)&frame, tmp));
  strcat(out, "&event_input=");
  strcat(out, itoa_u((uintptr_t)&event_input, tmp));
  strcat(out, "&output=");
  strcat(out, itoa_u((uintptr_t)&output, tmp));
  strcat(out, "&active_voices=");
  strcat(out, itoa_u((uintptr_t)&engine->active_voices, tmp));
  strcat(out, "&context_offset=");
  strcat(out, itoa_u((uintptr_t)&engine->context_offset, tmp));
  strcat(out, "&doughtime=");
  strcat(out, itoa_u((uintptr_t)&doughtime, tmp));
  js_init(out);
#endif // CLANGWASM
}

void dough_init(int sample_rate, int max_voices, int max_orbits, int max_delay_time, int max_pcm, int max_events, int max_files)
{
  dough_engine_init(&engine, sample_rate, max_voices, max_orbits, max_delay_time, max_pcm, max_events, max_files);
}

void dough_reset(void) { reset_event(&p); }

enum
{
  PCM_FAIL = -1,
  PCM_NOT_LOADED = -2
};

int dough_fits_pcm(int file_index, int size)
{
  if (file_index >= engine.max_files || file_index < 0)
  {
#if DOUGH_LOG
    printf("cannot register pcm: out of bounds. make sure 0 <= file_index < max_files\n");
#endif // DOUGH_LOG
    return PCM_FAIL;
  }
  if (engine.pcm_offset + size > engine.max_pcm)
  {
#if DOUGH_LOG
    printf("cannot register pcm: we're full. increase max_pcm in dough_init\n");
#endif // DOUGH_LOG
    return PCM_FAIL;
  }
  return engine.pcm_offset;
}

int dough_register_pcm(int file_index, const float *pcm, int frames, int channels, float freq)
{

  int size = frames * channels;
  int offset = dough_fits_pcm(file_index, size);
  if (offset == PCM_FAIL)
    return PCM_FAIL;
  float *dest = engine.pcm + offset;
  size_t bytes = size * sizeof(float);
  if (dest != pcm)
    memcpy(dest, pcm, bytes);
  engine.pcm_offset += size;
  // init file
  FileSource *file = &engine.files[file_index];
  file->pcm = dest;
  file->frames = frames;
  file->channels = channels;
  file->freq = freq;
  return 0;
}

#if DOUGH_MINIAUDIO
// https://garten.salat.dev/133-miniaudio/
#include "./libs/miniaudio.h"
#include <sys/time.h>

ma_device dough_device;
int64_t dough_frame = 0;
double dough_origin;

static double get_wallclock()
{
  struct timeval tv;
  gettimeofday(&tv, NULL);
  return tv.tv_sec + tv.tv_usec / (double)1000000.0;
}

static void data_callback(ma_device *pDevice, void *pOutput, const void *pInput, ma_uint32 frameCount)
{
  float *output = (float *)pOutput;
  double audio_time = (dough_frame / (double)engine.sr);
  double wall = get_wallclock();

  double deprivation = wall - dough_origin - audio_time;
  if (fabs(deprivation) > 0.02)
  {
    // if the offset between clocks is too high, we jump to make them equal again
    // this mostly happens when the system sleeps
    // this might also happen with normal clock drift but hopefully not too often
    // todo: add smoothing like ClockBridge in strudel
    dough_origin += deprivation;
#if DOUGH_LOG
    printf("shift time origin by %.1f s\n", deprivation);
#endif // DOUGH_LOG
  }

  double time = dough_origin + audio_time;
  dough_render(output, frameCount, time);
  dough_frame += frameCount;
}

int dough_start_audio(void)
{

  ma_device_config config = ma_device_config_init(ma_device_type_playback);
  config.playback.format = ma_format_f32;
  config.playback.channels = CHANNELS;
  config.sampleRate = engine.sr;
  config.dataCallback = data_callback;

  if (ma_device_init(NULL, &config, &dough_device) != MA_SUCCESS)
  {
    return -1; // init error
  }

  dough_origin = get_wallclock();

  ma_device_start(&dough_device);

  printf("miniaudio started...\n");

  return 0;
}

void dough_stop_audio(void)
{
  ma_device_uninit(&dough_device);
}

int load_local_sample(char *name, float freq)
{
  return -1;
}
void load_local_samples()
{
}

#endif // DOUGH_MINIAUDIO

#if DOUGH_PORTAUDIO

#include <portaudio.h>
#include <sys/time.h>
#define FRAMES_PER_BUFFER 256

double pa_time_origin;
static PaStream *pa_stream;

// mostly boilerplate from here
// adapted from https://files.portaudio.com/docs/v19-doxydocs/paex__sine_8c_source.html

static int DSPCallback(const void *inputBuffer, void *outputBuffer,
                       unsigned long framesPerBuffer,
                       const PaStreamCallbackTimeInfo *timeInfo,
                       PaStreamCallbackFlags statusFlags,
                       void *userData)
{
  float *output = (float *)outputBuffer;
  double time = timeInfo->outputBufferDacTime - pa_time_origin; // thanks dirt
  dough_render(output, framesPerBuffer, time);
  return paContinue;
}

int dough_start_audio(void)
{
  PaStreamParameters outputParameters;
  PaError err;

  err = Pa_Initialize();
  if (err != paNoError)
    goto error;
  outputParameters.device = Pa_GetDefaultOutputDevice(); /* default output device */
  if (outputParameters.device == paNoDevice)
  {
    fprintf(stderr, "Error: No default output device.\n");
    goto error;
  }
  outputParameters.channelCount = 2;         /* stereo output */
  outputParameters.sampleFormat = paFloat32; /* 32 bit floating point output */
  outputParameters.suggestedLatency = Pa_GetDeviceInfo(outputParameters.device)->defaultLowOutputLatency;
  outputParameters.hostApiSpecificStreamInfo = NULL;

  err = Pa_OpenStream(&pa_stream,
                      NULL, /* no input */
                      &outputParameters,
                      engine.sr,
                      FRAMES_PER_BUFFER,
                      paClipOff, /* we won't output out of range samples so don't bother clipping them */
                      DSPCallback,
                      NULL); // user data
  if (err != paNoError)
    goto error;

  // time origin
  struct timeval tv;
  gettimeofday(&tv, NULL);
  double now = tv.tv_sec + tv.tv_usec / 1000000.0;
  pa_time_origin = Pa_GetStreamTime(pa_stream) - now;
  // ^ taken from dirt

  err = Pa_StartStream(pa_stream);
  if (err != paNoError)
    goto error;

  return 0;
error:
  Pa_Terminate();
  fprintf(stderr, "An error occurred while using the portaudio stream\n");
  fprintf(stderr, "Error number: %d\n", err);
  fprintf(stderr, "Error message: %s\n", Pa_GetErrorText(err));
  return -1;
}

void dough_stop_audio(void)
{
  if (pa_stream)
  {
    Pa_StopStream(pa_stream);
    Pa_CloseStream(pa_stream);
    pa_stream = NULL;
  }
  Pa_Terminate();
}

/** taken from dirt: */
#include <sndfile.h>
#include <samplerate.h>

#define MAX_PATH 128
typedef struct
{
  char name[64];
  char path[MAX_PATH];
  float freq;
  int file_index; // local_files index or PCM_FAIL / PCM_NOT_LOADED
} LocalFile;

// Local sample definitions
LocalFile local_files[64];
int local_files_count;

int load_file(LocalFile *lf)
{
  // Load the sndfile
  SF_INFO info;
  SNDFILE *sndfile;
  printf("Loading sample %s...\n", lf->path);
  if ((sndfile = (SNDFILE *)sf_open(lf->path, SFM_READ, &info)) == NULL)
  {
    lf->file_index = PCM_FAIL;
    return PCM_FAIL;
  }
  if (info.channels > CHANNELS)
  {
    sf_close(sndfile);
    lf->file_index = PCM_FAIL;
    return PCM_FAIL;
  }
  float *pcm = (float *)calloc(1, sizeof(float) * info.frames * info.channels);
  sf_count_t count = sf_read_float(sndfile, pcm, info.frames * info.channels);
  sf_close(sndfile);
  if (count != info.frames * info.channels)
  {
    free(pcm);
    lf->file_index = PCM_FAIL;
    return PCM_FAIL;
  }

  // Fix samplerate
  int frames = info.frames;
  if (info.samplerate != (int)engine.sr)
  {
    SRC_DATA data;
    data.src_ratio = engine.sr / (float)info.samplerate;
    int max_output_frames = info.frames * data.src_ratio + 32;
    data.data_in = pcm;
    data.input_frames = info.frames;
    data.data_out = (float *)calloc(1, sizeof(float) * max_output_frames * info.channels);
    data.output_frames = max_output_frames;
    src_simple(&data, SRC_SINC_BEST_QUALITY, info.channels);
    free(pcm);
    pcm = data.data_out;
    frames = data.output_frames_gen;
  }
  int file_index = lf - local_files; // :3
  printf("Loaded %s: %d samples\n", lf->name, frames);
  int status = dough_register_pcm(file_index, pcm, frames, info.channels, lf->freq);
  free(pcm);
  if (status == PCM_FAIL)
  {
    lf->file_index = PCM_FAIL;
    return PCM_FAIL;
  }
  lf->file_index = file_index;
  return 0;
}

void load_local_samples()
{
  local_files_count = 0;
  FILE *file = fopen("samples.txt", "r");
  if (file == NULL)
    return;
  char line[1024];
  while (fgets(line, sizeof(line), file))
  {
    // TODO: handle local file definition...
    // e.g. 'piano 65.46 samples/piano_C2v8.mp3' would be:
    LocalFile *local = &local_files[local_files_count++];
    strcpy(local->name, "piano");
    local->freq = 65.46;
    local->file_index = PCM_NOT_LOADED;
    strcpy(local->path, "samples/piano_C2v8.mp3");
  }
  fclose(file);
}
int load_local_sample(char *name, float freq)
{
  LocalFile *closest = NULL;
  for (int i = 0; i < local_files_count; i++)
  {
    LocalFile *file = &local_files[i];
    if (!strcmp(file->name, name))
      if (closest == NULL || (fabsf(freq - file->freq) < fabsf(freq - closest->freq)))
        closest = file;
  }
  if (closest)
  {
    if (closest->file_index == PCM_NOT_LOADED)
      load_file(closest);
    if (closest->file_index == PCM_FAIL)
      return PCM_FAIL;
    return closest->file_index;
  }
  return PCM_FAIL;
}

#endif // DOUGH_PORTAUDIO

#ifdef BENCH_DOUGH
#include <unistd.h>
int main(int argc, char **argv)
{
  int sr = 48000;
  int test_duration = sr * 4;
  int voice_count = 64;
  if (argc > 1)
    voice_count = atoi(argv[1]);
  dough_init(sr, voice_count, 2, 2, 0, 32, 0);
  float freq = 130.0;
  float buf[test_duration * CHANNELS];
  for (int i = 0; i < test_duration; i++)
  {
    if (i % 10000 == 0)
    {
      reset_event(&p);
      p.freq = freq;
      freq += freq / 10.0;
      p.duration = 0.05;
      p.attack = 0.02;
      p.release = 0.5;
      p.delay = 1;
      p.delayfeedback = 0.2;
      p.gain = 0.5;
      p.orbit = 1;
      play_event(&engine, &p);
    }
    gen_sample(&engine, buf, i);
  }
  // validate with:
  // cc -std=gnu99 -O2 -DDOUGH_IMPLEMENTATION -DBENCH_DOUGH=1 -x c dough.h -o dough-bench -lm && ./dough-bench | sox -t f32 -r 48000 -c 2 - -d
  // write(1, buf, sizeof(buf));
}

#endif // BENCH_DOUGH

#endif // DOUGH_IMPLEMENTATION
